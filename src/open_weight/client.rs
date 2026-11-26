use crate::{
    config::{self, Env},
    mcp::valid_name,
    runner::{Failure, Metrics},
};
use anyhow::{Context, Result, ensure};
use reqwest::{Request, Response, StatusCode, Url, header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json, value::RawValue};
use std::time::Instant;

#[cfg(test)]
type Fixture = dyn Fn(Request) -> std::result::Result<Response, Failure> + Send + Sync;

pub(super) struct Client {
    http: reqwest::Client,
    url: Url,
    auth: header::HeaderValue,
    model: String,
    max_tokens: u32,
    temperature: f64,
    chat_template_kwargs: Value,
    request_limit: usize,
    response_limit: usize,
    arguments_limit: usize,
    #[cfg(test)]
    fixture: Option<Box<Fixture>>,
}

pub(super) enum Turn {
    /// Parsed calls plus the assistant message to append to the conversation verbatim.
    Calls(Vec<Call>, Value),
    Done,
}

pub(super) struct Call {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Serialize)]
struct ChatRequest<'a, M> {
    model: &'a str,
    messages: &'a [M],
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a RawValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<&'a Value>,
    stream: bool,
    max_tokens: u32,
    temperature: f64,
    chat_template_kwargs: &'a Value,
}

#[derive(Deserialize)]
struct Completion {
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Deserialize)]
struct CompletionTokensDetails {
    reasoning_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct Choice {
    finish_reason: String,
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    role: String,
    content: Option<String>,
    refusal: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
    /// vLLM's reasoning parser output; older releases name it `reasoning_content`.
    reasoning_content: Option<String>,
    reasoning: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: Function,
}

#[derive(Deserialize, Serialize)]
struct Function {
    name: String,
    arguments: String,
}

impl Client {
    pub(super) fn from_env(env: Env) -> Result<Self> {
        Ok(Self {
            http: config::http_client(
                config::seconds(env, "VLLM_CONNECT_TIMEOUT_SECONDS")?,
                config::seconds(env, "VLLM_TIMEOUT_SECONDS")?,
            )?,
            url: endpoint(&config::required(env, "VLLM_URL")?)?,
            auth: bearer(&config::required(env, "VLLM_API_KEY")?)?,
            model: model(config::required(env, "VLLM_MODEL")?)?,
            max_tokens: config::positive(env, "VLLM_MAX_TOKENS")?,
            temperature: temperature(&config::required(env, "VLLM_TEMPERATURE")?)?,
            chat_template_kwargs: template_kwargs(&config::required(
                env,
                "VLLM_CHAT_TEMPLATE_KWARGS",
            )?)?,
            request_limit: config::positive(env, "VLLM_REQUEST_LIMIT_BYTES")?,
            response_limit: config::positive(env, "VLLM_RESPONSE_LIMIT_BYTES")?,
            arguments_limit: config::positive(env, "VLLM_TOOL_ARGUMENTS_LIMIT_BYTES")?,
            #[cfg(test)]
            fixture: None,
        })
    }

    pub(super) async fn complete(
        &self,
        messages: &[Box<RawValue>],
        tools: &RawValue,
        metrics: &Metrics,
    ) -> std::result::Result<Turn, Failure> {
        let start = Instant::now();
        let result = self.post(messages, Some(tools), None).await;
        metrics.inference(start.elapsed());
        parse_turn(result?, self.arguments_limit, metrics)
    }

    /// One tool-free completion whose content must match `response_format`; returns the text.
    pub(super) async fn answer(
        &self,
        messages: &[Value],
        response_format: &Value,
    ) -> std::result::Result<String, Failure> {
        let completion = self.post(messages, None, Some(response_format)).await?;
        let [choice]: [Choice; 1] = completion
            .choices
            .try_into()
            .map_err(|_| Failure::Inference)?;
        let Message {
            role,
            content,
            refusal,
            tool_calls,
            ..
        } = choice.message;
        match content {
            Some(text)
                if choice.finish_reason == "stop"
                    && role == "assistant"
                    && refusal.is_none_or(|text| text.is_empty())
                    && tool_calls.is_none_or(|calls| calls.is_empty()) =>
            {
                Ok(text)
            }
            _ => Err(Failure::Inference),
        }
    }

    async fn post(
        &self,
        messages: &[impl Serialize],
        tools: Option<&RawValue>,
        response_format: Option<&Value>,
    ) -> std::result::Result<Completion, Failure> {
        let body = serde_json::to_vec(&ChatRequest {
            model: &self.model,
            messages,
            tools,
            tool_choice: tools.map(|_| "auto"),
            response_format,
            stream: false,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            chat_template_kwargs: &self.chat_template_kwargs,
        })
        .map_err(|_| Failure::Runner)?;
        if body.len() > self.request_limit {
            return Err(Failure::Runner);
        }
        let request = self
            .http
            .post(self.url.clone())
            .header(header::AUTHORIZATION, self.auth.clone())
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .build()
            .map_err(|_| Failure::Runner)?;
        read_completion(self.send(request).await?, self.response_limit).await
    }

    async fn send(&self, request: Request) -> std::result::Result<Response, Failure> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return fixture(request);
        }
        self.http
            .execute(request)
            .await
            .map_err(|_| Failure::Inference)
    }
}

async fn read_completion(
    mut response: Response,
    limit: usize,
) -> std::result::Result<Completion, Failure> {
    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        return Err(Failure::InferenceThrottled);
    }
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|bytes| bytes > limit as u64)
        || !response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_some_and(|content_type| {
                content_type.to_str().is_ok_and(|value| {
                    value.split(';').next().is_some_and(|media_type| {
                        media_type.trim().eq_ignore_ascii_case("application/json")
                    })
                })
            })
    {
        return Err(Failure::Inference);
    }
    let mut body = Vec::with_capacity(response.content_length().unwrap_or(0) as usize);
    while let Some(chunk) = response.chunk().await.map_err(|_| Failure::Inference)? {
        if chunk.len() > limit - body.len() {
            return Err(Failure::Inference);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| Failure::Inference)
}

fn parse_turn(
    completion: Completion,
    arguments_limit: usize,
    metrics: &Metrics,
) -> std::result::Result<Turn, Failure> {
    let usage = completion.usage;
    if usage
        .prompt_tokens
        .checked_add(usage.completion_tokens)
        .is_none()
    {
        return Err(Failure::Inference);
    }
    metrics.inference_tokens(usage.prompt_tokens, usage.completion_tokens);
    let [choice]: [Choice; 1] = completion
        .choices
        .try_into()
        .map_err(|_| Failure::Inference)?;
    let Message {
        role,
        content,
        refusal,
        tool_calls,
        reasoning_content,
        reasoning,
    } = choice.message;
    if role != "assistant" || refusal.is_some_and(|text| !text.is_empty()) {
        return Err(Failure::Inference);
    }
    // OpenAI chat completions omit `tool_calls` when the model calls nothing.
    let calls = tool_calls.unwrap_or_default();
    // Reasoning and any text beside tool calls never enter history: only the calls are kept.
    let text = |value: &Option<String>| value.as_deref().is_some_and(|text| !text.is_empty());
    let dropped =
        text(&reasoning_content) || text(&reasoning) || (!calls.is_empty() && text(&content));
    let reasoning_tokens = usage
        .completion_tokens_details
        .and_then(|details| details.reasoning_tokens)
        .unwrap_or(0);
    metrics.reasoning_discarded(u64::from(dropped), reasoning_tokens);
    match choice.finish_reason.as_str() {
        "stop"
            if calls.is_empty()
                && content
                    .as_deref()
                    .is_some_and(|text| !text.trim().is_empty()) =>
        {
            return Ok(Turn::Done);
        }
        // The engine bounds the count by the session's remaining tool-call budget.
        "tool_calls" if !calls.is_empty() => {}
        _ => return Err(Failure::Inference),
    }
    let mut parsed: Vec<Call> = Vec::with_capacity(calls.len());
    for ToolCall { id, kind, function } in &calls {
        // The id length cap is a parse guard; ids echo back into the next request.
        if kind != "function"
            || !(1..=128).contains(&id.len())
            || !id.bytes().all(|byte| byte.is_ascii_graphic())
            || parsed.iter().any(|call| &call.id == id)
            || !valid_name(&function.name)
            || function.arguments.len() > arguments_limit
        {
            return Err(Failure::Inference);
        }
        let args =
            serde_json::from_str::<Value>(&function.arguments).map_err(|_| Failure::Inference)?;
        if !args.is_object() {
            return Err(Failure::Inference);
        }
        parsed.push(Call {
            id: id.clone(),
            name: function.name.clone(),
            args,
        });
    }
    let message = json!({"role": "assistant", "content": null, "tool_calls": calls});
    Ok(Turn::Calls(parsed, message))
}

fn endpoint(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| anyhow::anyhow!("VLLM_URL is invalid"))?;
    ensure!(
        url.scheme() == "https"
            && url.has_host()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            // OpenAI-compatible chat completions route.
            && url.path().ends_with("/v1/chat/completions"),
        "VLLM_URL must be an HTTPS /v1/chat/completions endpoint without credentials, query, or fragment"
    );
    Ok(url)
}

/// The length caps are sanity bounds on header values, not deployment choices.
fn bearer(token: &str) -> Result<header::HeaderValue> {
    ensure!(
        (1..=4096).contains(&token.len()) && token.bytes().all(|byte| byte.is_ascii_graphic()),
        "VLLM_API_KEY must contain 1 to 4096 visible ASCII bytes"
    );
    let mut auth = header::HeaderValue::from_str(&format!("Bearer {token}"))?;
    auth.set_sensitive(true);
    Ok(auth)
}

fn model(model: String) -> Result<String> {
    ensure!(
        (1..=256).contains(&model.len()) && model.bytes().all(|byte| byte.is_ascii_graphic()),
        "VLLM_MODEL must contain 1 to 256 visible ASCII bytes"
    );
    Ok(model)
}

/// The OpenAI chat completions API accepts temperatures from 0 to 2.
fn temperature(value: &str) -> Result<f64> {
    value
        .parse()
        .ok()
        .filter(|temperature| (0.0..=2.0).contains(temperature))
        .context("VLLM_TEMPERATURE must be a number from 0.0 to 2.0")
}

/// Passed to the model's chat template verbatim, for example a reasoning switch.
fn template_kwargs(value: &str) -> Result<Value> {
    serde_json::from_str::<Value>(value)
        .ok()
        .filter(Value::is_object)
        .context("VLLM_CHAT_TEMPLATE_KWARGS must be a JSON object")
}

#[cfg(test)]
#[path = "client_tests.rs"]
pub(super) mod tests;
