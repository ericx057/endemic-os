use crate::{
    runner::{Failure, Metrics, ToolTrace},
    target::{self, Tool, TraceContext, Wire},
};
use reqwest::{Method, RequestBuilder, Response, StatusCode, Url, header};
use serde_json::{Value, json};

/// MCP specification revision this client speaks.
const PROTOCOL: &str = "2025-11-25";
/// Parse guard: tools/list pages followed before the tool list must be complete.
const MAX_TOOL_PAGES: usize = 4;

#[cfg(test)]
type Fixture = dyn Fn(reqwest::Request) -> Result<Response, Failure> + Send + Sync;

pub struct Target {
    wire: Wire,
    url: Url,
    auth: header::HeaderValue,
    /// None until `discover`; then the run's tool set, which every session must find.
    allowed: Option<Vec<String>>,
    /// Sorted names of the read-only tools, dispatched in parallel.
    parallel: Vec<String>,
    #[cfg(test)]
    fixture: Option<Box<Fixture>>,
}

pub struct Session<'a> {
    target: &'a Target,
    session_id: Option<header::HeaderValue>,
    context: TraceContext,
    next_id: u64,
    tools: Vec<Tool>,
    trace: Option<ToolTrace<'a>>,
}

impl Target {
    pub(crate) fn new(url: Url, auth: header::HeaderValue, wire: Wire) -> Self {
        Self {
            wire,
            url,
            auth,
            allowed: None,
            parallel: Vec::new(),
            #[cfg(test)]
            fixture: None,
        }
    }

    /// Lists every tool once and fixes the run's tool set: all listed tools are allowed, and
    /// those annotated `readOnlyHint: true` are dispatched in parallel.
    pub(crate) async fn discover(mut self) -> Result<(Self, Vec<Tool>), Failure> {
        let mut session = self.connect().await?;
        let tools = std::mem::take(&mut session.tools);
        session.close().await?;
        self.allowed = Some(tools.iter().map(|tool| tool.name.clone()).collect());
        self.parallel = tools
            .iter()
            .filter(|tool| tool.read_only)
            .map(|tool| tool.name.clone())
            .collect();
        self.parallel.sort_unstable();
        Ok((self, tools))
    }

    pub async fn connect(&self) -> Result<Session<'_>, Failure> {
        let context = TraceContext::new();
        let request = self.request(Method::POST, &context).json(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                    "protocolVersion": PROTOCOL,
                    "capabilities": {},
                    "clientInfo": {"name": "endemic", "version": env!("CARGO_PKG_VERSION")}
                }
        }));
        let response = self.send(request).await?;
        let mut session_id = response.headers().get("mcp-session-id").cloned();
        if let Some(id) = &mut session_id {
            // MCP transport: session ids are visible ASCII; the length cap is a parse guard.
            if id.is_empty()
                || id.len() > 256
                || !id.as_bytes().iter().all(|b| (0x21..=0x7e).contains(b))
            {
                return Err(Failure::Target);
            }
            id.set_sensitive(true);
        }
        let info = rpc_result(response, 1, self.wire.response_limit).await?;
        if info["protocolVersion"] != PROTOCOL || !info["capabilities"]["tools"].is_object() {
            return Err(Failure::Target);
        }

        let mut session = Session {
            target: self,
            session_id,
            context,
            next_id: 2,
            tools: Vec::new(),
            trace: None,
        };
        if let Err(error) = session.discover().await {
            // The discovery failure is the error to report; closing is cleanup only.
            let _ = session.close().await;
            return Err(error);
        }
        Ok(session)
    }

    fn request(&self, method: Method, context: &TraceContext) -> RequestBuilder {
        self.wire
            .client
            .request(method, self.url.clone())
            .header(header::AUTHORIZATION, self.auth.clone())
            .header("traceparent", context.traceparent())
            .header(header::ACCEPT, "application/json, text/event-stream")
    }

    async fn send(&self, request: RequestBuilder) -> Result<Response, Failure> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return fixture(request.build().map_err(|_| Failure::Target)?);
        }
        request.send().await.map_err(|_| Failure::Target)
    }
}

impl<'a> Session<'a> {
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    pub(crate) fn observe(&mut self, metrics: &'a Metrics) {
        self.trace = metrics.trace(self.context.trace_id());
    }

    pub(crate) fn can_parallel(&self, name: &str) -> bool {
        self.target
            .parallel
            .binary_search_by(|tool| tool.as_str().cmp(name))
            .is_ok()
    }

    #[cfg(test)]
    async fn call(&mut self, name: &str, args: Value) -> Result<Value, Failure> {
        let (request, id) = self.prepare_tool(name, args)?;
        self.send_tool(request, id).await
    }

    pub(crate) async fn call_group(
        &mut self,
        calls: Vec<(String, Value)>,
        metrics: &Metrics,
    ) -> Result<Vec<Value>, Failure> {
        target::check_batch(&calls, |name| self.can_parallel(name))?;
        let mut prepared = Vec::with_capacity(calls.len());
        for (name, args) in calls {
            let (request, id) = self.prepare_tool(&name, args)?;
            prepared.push((name, request, id));
        }
        let names = prepared.iter().map(|(name, _, _)| name.as_str());
        target::record_batch(metrics, &mut self.trace, names);
        let session = &*self;
        let sends = prepared
            .into_iter()
            .map(|(name, request, id)| (name, session.send_tool(request, id)));
        target::dispatch(metrics, sends).await
    }

    fn prepare_tool(&mut self, name: &str, args: Value) -> Result<(RequestBuilder, u64), Failure> {
        if !self.tools.iter().any(|tool| tool.name == name) || !args.is_object() {
            return Err(Failure::Runner);
        }
        self.prepare_rpc("tools/call", json!({"name": name, "arguments": args}))
    }

    async fn send_tool(&self, request: RequestBuilder, id: u64) -> Result<Value, Failure> {
        let response = self.target.send(request).await?;
        let result = rpc_result(response, id, self.target.wire.response_limit).await?;
        if !result["content"].is_array()
            || result
                .get("isError")
                .is_some_and(|is_error| is_error != false)
        {
            return Err(Failure::Target);
        }
        Ok(result)
    }

    pub async fn close(self) -> Result<(), Failure> {
        if self.session_id.is_some() {
            let response = self.target.send(self.request(Method::DELETE)).await?;
            if !response.status().is_success()
                && response.status() != StatusCode::METHOD_NOT_ALLOWED
            {
                return Err(Failure::Target);
            }
        }
        Ok(())
    }

    async fn discover(&mut self) -> Result<(), Failure> {
        let request = self
            .request(Method::POST)
            .json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        let response = self.target.send(request).await?;
        if response.status() != StatusCode::ACCEPTED {
            return Err(Failure::Target);
        }
        let mut params = json!({});
        let target = self.target;
        let allowed = target.allowed.as_ref();
        for _ in 0..MAX_TOOL_PAGES {
            let page = self.rpc("tools/list", params).await?;
            for tool in page["tools"].as_array().ok_or(Failure::Target)? {
                let name = tool["name"].as_str().ok_or(Failure::Target)?;
                if allowed.is_some_and(|allowed| !allowed.iter().any(|known| known == name)) {
                    continue;
                }
                if !valid_name(name)
                    || tool["inputSchema"]["type"] != "object"
                    || self.tools.iter().any(|tool| tool.name == name)
                {
                    return Err(Failure::Target);
                }
                self.tools.push(Tool {
                    name: name.to_owned(),
                    description: tool["description"].as_str().map(str::to_owned),
                    schema: tool["inputSchema"].clone(),
                    read_only: tool["annotations"]["readOnlyHint"] == true,
                });
            }
            let complete = allowed.map(|allowed| self.tools.len() == allowed.len());
            let cursor = page["nextCursor"]
                .as_str()
                .filter(|cursor| !cursor.is_empty());
            match (complete, cursor) {
                (Some(true), _) | (None, None) => return Ok(()),
                (_, Some(cursor)) if cursor.len() <= 1024 => params = json!({"cursor": cursor}),
                _ => return Err(Failure::Target),
            }
        }
        Err(Failure::Target)
    }

    async fn rpc(&mut self, method: &str, params: Value) -> Result<Value, Failure> {
        let (request, id) = self.prepare_rpc(method, params)?;
        let response = self.target.send(request).await?;
        rpc_result(response, id, self.target.wire.response_limit).await
    }

    fn prepare_rpc(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<(RequestBuilder, u64), Failure> {
        let id = self.next_id;
        self.next_id += 1;
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }))
        .map_err(|_| Failure::Runner)?;
        if body.len() > self.target.wire.request_limit {
            return Err(Failure::Runner);
        }
        let request = self
            .request(Method::POST)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body);
        Ok((request, id))
    }

    fn request(&self, method: Method) -> RequestBuilder {
        let request = self
            .target
            .request(method, &self.context)
            .header("mcp-protocol-version", PROTOCOL);
        match &self.session_id {
            Some(id) => request.header("mcp-session-id", id.clone()),
            None => request,
        }
    }
}

/// Tool and function names as the OpenAI tools API accepts them: 1-64 of `[A-Za-z0-9_-]`.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

async fn rpc_result(mut response: Response, id: u64, limit: usize) -> Result<Value, Failure> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > limit as u64)
    {
        return Err(Failure::Target);
    }
    let kind = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .and_then(|kind| kind.split(';').next())
        .ok_or(Failure::Target)?
        .trim();
    let streaming = kind.eq_ignore_ascii_case("text/event-stream");
    if !streaming && !kind.eq_ignore_ascii_case("application/json") {
        return Err(Failure::Target);
    }
    let mut body = Vec::with_capacity(response.content_length().unwrap_or(0) as usize);
    let mut event = Vec::new();
    let (mut line_start, mut scanned) = (0, 0);
    while let Some(chunk) = response.chunk().await.map_err(|_| Failure::Target)? {
        if body.len() + chunk.len() > limit {
            return Err(Failure::Target);
        }
        body.extend_from_slice(&chunk);
        if !streaming {
            continue;
        }
        while scanned < body.len() {
            let byte = body[scanned];
            scanned += 1;
            if byte != b'\n' && byte != b'\r' {
                continue;
            }
            let mut line = &body[line_start..scanned - 1];
            if line_start == 0 {
                line = line.strip_prefix(b"\xef\xbb\xbf").unwrap_or(line);
            }
            line_start = scanned;
            if byte == b'\n' && scanned >= 2 && body[scanned - 2] == b'\r' {
                continue;
            }
            if let Some(data) = line.strip_prefix(b"data:") {
                event.extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
                event.push(b'\n');
            } else if line.is_empty() && !event.is_empty() {
                if let Some(result) = decode_result(&event, id)? {
                    return Ok(result);
                }
                event.clear();
            }
        }
    }
    if streaming {
        return Err(Failure::Target);
    }
    decode_result(&body, id)?.ok_or(Failure::Target)
}

fn decode_result(body: &[u8], id: u64) -> Result<Option<Value>, Failure> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let mut message: Value = serde_json::from_slice(body).map_err(|_| Failure::Target)?;
    // JSON-RPC 2.0 envelope.
    if message["jsonrpc"] != "2.0" {
        return Err(Failure::Target);
    }
    if message.get("id").is_none() && message["method"].is_string() {
        return Ok(None);
    }
    if message["id"].as_u64() != Some(id)
        || message.get("error").is_some()
        || !message["result"].is_object()
    {
        return Err(Failure::Target);
    }
    Ok(Some(message["result"].take()))
}

#[cfg(test)]
mod tests;
