use crate::{
    runner::{Failure, Metrics, ToolTrace},
    target::{self, Tool, TraceContext, Wire},
};
use reqwest::{RequestBuilder, Response, Url, header};
use serde_json::{Map, Value, json};

mod openapi;
use openapi::Operation;

pub struct Target {
    wire: Wire,
    url: Url,
    auth: header::HeaderValue,
    tools: Vec<Tool>,
    operations: Vec<Operation>,
}

pub struct Session<'a> {
    target: &'a Target,
    context: TraceContext,
    trace: Option<ToolTrace<'a>>,
}

impl Target {
    pub(crate) fn new(
        url: Url,
        auth: header::HeaderValue,
        spec: &Value,
        wire: Wire,
    ) -> anyhow::Result<Self> {
        let (tools, operations) = openapi::operations(spec)?.into_iter().unzip();
        Ok(Self {
            wire,
            url,
            auth,
            tools,
            operations,
        })
    }

    pub(crate) fn tools(&self) -> &[Tool] {
        &self.tools
    }

    pub(crate) fn labels(&self) -> Vec<String> {
        self.operations
            .iter()
            .map(|operation| operation.label.clone())
            .collect()
    }

    pub async fn connect(&self) -> Result<Session<'_>, Failure> {
        Ok(Session {
            target: self,
            context: TraceContext::new(),
            trace: None,
        })
    }
}

impl<'a> Session<'a> {
    pub fn tools(&self) -> &[Tool] {
        &self.target.tools
    }

    pub(crate) fn observe(&mut self, metrics: &'a Metrics) {
        self.trace = metrics.trace(self.context.trace_id());
    }

    pub(crate) fn can_parallel(&self, name: &str) -> bool {
        self.tools()
            .iter()
            .any(|tool| tool.name == name && tool.read_only)
    }

    pub(crate) async fn call_group(
        &mut self,
        calls: Vec<(String, Value)>,
        metrics: &Metrics,
    ) -> Result<Vec<Value>, Failure> {
        target::check_batch(&calls, |name| self.can_parallel(name))?;
        let mut prepared = Vec::with_capacity(calls.len());
        for (name, args) in calls {
            let (operation, request) = self.prepare(&name, args)?;
            prepared.push((name, operation, request));
        }
        let names = prepared.iter().map(|(name, _, _)| name.as_str());
        target::record_batch(metrics, &mut self.trace, names);
        let limit = self.target.wire.response_limit;
        let sends = prepared
            .into_iter()
            .map(|(_, operation, request)| (operation.label.clone(), send(request, limit)));
        target::dispatch(metrics, sends).await
    }

    fn prepare(&self, name: &str, args: Value) -> Result<(&'a Operation, RequestBuilder), Failure> {
        let target = self.target;
        let index = target.tools.iter().position(|tool| tool.name == name);
        let operation = &target.operations[index.ok_or(Failure::Runner)?];
        let Value::Object(args) = args else {
            return Err(Failure::Runner);
        };
        if args
            .keys()
            .any(|key| !["path", "query", "body"].contains(&key.as_str()))
        {
            return Err(Failure::Runner);
        }
        let url = url(
            &target.url,
            operation,
            object(args.get("path"))?,
            object(args.get("query"))?,
        )?;
        let request = target
            .wire
            .client
            .request(operation.method.clone(), url)
            .header(header::AUTHORIZATION, target.auth.clone())
            .header("traceparent", self.context.traceparent())
            .header(header::ACCEPT, "application/json");
        match args.get("body") {
            Some(body) if operation.body => {
                let body = serde_json::to_vec(body).map_err(|_| Failure::Runner)?;
                if body.len() > target.wire.request_limit {
                    return Err(Failure::Runner);
                }
                let request = request.header(header::CONTENT_TYPE, "application/json");
                Ok((operation, request.body(body)))
            }
            Some(_) => Err(Failure::Runner),
            None => Ok((operation, request)),
        }
    }

    pub async fn close(self) -> Result<(), Failure> {
        Ok(())
    }
}

fn object(value: Option<&Value>) -> Result<Option<&Map<String, Value>>, Failure> {
    match value {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(_) => Err(Failure::Runner),
    }
}

fn url(
    base: &Url,
    operation: &Operation,
    path: Option<&Map<String, Value>>,
    query: Option<&Map<String, Value>>,
) -> Result<Url, Failure> {
    let mut url = base.clone();
    {
        let mut segments = url.path_segments_mut().map_err(|_| Failure::Runner)?;
        segments.pop_if_empty();
        for segment in operation.path.trim_start_matches('/').split('/') {
            segments.push(&fill(segment, path)?);
        }
    }
    for (name, value) in query.into_iter().flatten() {
        if !operation.query.contains(name) {
            return Err(Failure::Runner);
        }
        url.query_pairs_mut().append_pair(name, &scalar(value)?);
    }
    Ok(url)
}

fn fill(segment: &str, path: Option<&Map<String, Value>>) -> Result<String, Failure> {
    let (mut filled, mut rest) = (String::new(), segment);
    while let Some(start) = rest.find('{') {
        let end = start + rest[start..].find('}').ok_or(Failure::Runner)?;
        let value = path.and_then(|path| path.get(&rest[start + 1..end]));
        let value = scalar(value.ok_or(Failure::Runner)?)?;
        if matches!(value.as_str(), "" | "." | "..") {
            return Err(Failure::Runner);
        }
        filled.push_str(&rest[..start]);
        filled.push_str(&value);
        rest = &rest[end + 1..];
    }
    filled.push_str(rest);
    Ok(filled)
}

fn scalar(value: &Value) -> Result<String, Failure> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        _ => Err(Failure::Runner),
    }
}

async fn send(request: RequestBuilder, limit: usize) -> Result<Value, Failure> {
    let mut response: Response = request.send().await.map_err(|_| Failure::Target)?;
    let status = response.status();
    if !status.is_success()
        || response
            .content_length()
            .is_some_and(|size| size > limit as u64)
    {
        return Err(Failure::Target);
    }
    let mut body = Vec::with_capacity(response.content_length().unwrap_or(0) as usize);
    while let Some(chunk) = response.chunk().await.map_err(|_| Failure::Target)? {
        if body.len() + chunk.len() > limit {
            return Err(Failure::Target);
        }
        body.extend_from_slice(&chunk);
    }
    let body = serde_json::from_slice::<Value>(&body)
        .unwrap_or_else(|_| String::from_utf8_lossy(&body).into_owned().into());
    Ok(json!({"status": status.as_u16(), "body": body}))
}

#[cfg(test)]
pub(crate) mod tests;
