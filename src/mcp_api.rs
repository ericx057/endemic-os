use super::{App, cancel_run, get_run, start_run};
use crate::{
    request::{Bearer, RunRequest},
    runner::Pacing,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

/// MCP specification revisions this server implements; LEGACY[0] is the newest
/// initialize-based revision, answered when a client requests one it does not know.
const LEGACY: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
const MODERN: &str = "2026-07-28";
const INSTRUCTIONS: &str = "Run managed agentic load tests against a staging target: Endemic discovers its tools, generates user tasks, and either holds a set number of agents or ramps agents until a stage fails or the ceiling is reached. Ask for operator confirmation before starting or cancelling a run.";

pub(super) async fn handle(
    State(app): State<App>,
    headers: HeaderMap,
    Json(mut message): Json<Value>,
) -> Response {
    let id = message
        .as_object_mut()
        .and_then(|object| object.remove("id"));
    let method = message["method"].as_str();
    if message["jsonrpc"] != "2.0"
        || method.is_none()
        || id
            .as_ref()
            .is_some_and(|id| !(id.is_string() || id.is_i64() || id.is_u64()))
    {
        return error(
            StatusCode::BAD_REQUEST,
            Value::Null,
            -32600,
            "Invalid Request",
        );
    }
    let Some(id) = id else {
        return match method {
            Some("notifications/initialized" | "notifications/cancelled") => StatusCode::ACCEPTED,
            _ => StatusCode::BAD_REQUEST,
        }
        .into_response();
    };
    let params = &message["params"];
    let version = headers
        .get("mcp-protocol-version")
        .map(|value| value.to_str().unwrap_or(""));
    let modern = version == Some(MODERN);
    if let Some(requested) = version.filter(|version| !modern && !LEGACY.contains(version)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "jsonrpc":"2.0", "id":id, "error":{
                    "code":-32022, "message":"Unsupported protocol version",
                    "data":{"supported":[MODERN, LEGACY[0], LEGACY[1], LEGACY[2]],"requested":requested}
                }
            })),
        )
            .into_response();
    }
    if modern {
        let header = |name| headers.get(name).and_then(|value| value.to_str().ok());
        if params["_meta"]["io.modelcontextprotocol/protocolVersion"] != MODERN
            || header("mcp-method") != method
            || (method == Some("tools/call")
                && (header("mcp-name").is_none() || header("mcp-name") != params["name"].as_str()))
        {
            return error(
                StatusCode::BAD_REQUEST,
                id,
                -32020,
                "MCP headers must match the request body",
            );
        }
    }
    let server = json!({"name":"endemic","version":env!("CARGO_PKG_VERSION")});
    let mut result = match method {
        Some("server/discover") if modern => json!({
            "supportedVersions":[MODERN], "capabilities":{"tools":{}}, "instructions":INSTRUCTIONS
        }),
        Some("initialize") if !modern => {
            let Some(requested) = params["protocolVersion"].as_str() else {
                return error(StatusCode::OK, id, -32602, "Expected protocolVersion");
            };
            let version = if LEGACY.contains(&requested) {
                requested
            } else {
                LEGACY[0]
            };
            json!({
                "protocolVersion":version, "serverInfo":server,
                "capabilities":{"tools":{}}, "instructions":INSTRUCTIONS
            })
        }
        Some("ping") if !modern => json!({}),
        Some("tools/list") => tools(),
        Some("tools/call") => match call(app, params).await {
            Ok(result) => {
                json!({"content":[{"type":"text","text":result.to_string()}],"structuredContent":result})
            }
            Err(CallError::Invalid) => {
                return error(
                    StatusCode::OK,
                    id,
                    -32602,
                    "Unknown tool or invalid arguments",
                );
            }
            Err(CallError::Run(message)) => json!({
                "isError":true, "content":[{"type":"text","text":message}]
            }),
        },
        _ => return error(StatusCode::NOT_FOUND, id, -32601, "Method not found"),
    };
    if modern {
        result["resultType"] = json!("complete");
        result["_meta"] = json!({"io.modelcontextprotocol/serverInfo":server});
    }
    Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
}

fn error(status: StatusCode, id: Value, code: i32, message: &str) -> Response {
    (
        status,
        Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})),
    )
        .into_response()
}

fn tools() -> Value {
    let text = |description| json!({"type":"string","description":description});
    let start = json!({
        "target_url": text("HTTPS URL of the staging MCP endpoint, or the HTTP API base URL when openapi_url is set."),
        "openapi_url": text("HTTPS URL of the HTTP API's OpenAPI 3 JSON document. Omit for an MCP target."),
        "bearer": text("Staging bearer token. Held in memory for this run only; never returned or logged."),
        "use_case": text("Optional, 1 to 4096 bytes: what an average user does, to steer the generated tasks."),
        "pacing": {"type":"object","additionalProperties":false,"description":"Optional; every field defaults to 0 or false, which starts all agents together with no pauses.","properties":{
            "start_spread_ms": {"type":"integer","minimum":0,"description":"Each agent starts at a random offset below this after the stage begins."},
            "think_min_ms": {"type":"integer","minimum":0,"description":"Shortest random pause between an agent's sessions."},
            "think_max_ms": {"type":"integer","minimum":0,"description":"Longest random pause between an agent's sessions."},
            "sync_sessions": {"type":"boolean","description":"Agents begin every session together, in waves."}
        }},
        "hold_agents": {"type":"integer","minimum":1,"description":"Optional: run one stage at exactly this many agents to check the target holds them. Omit to find the breaking point."},
    });
    let id = json!({"id":{"type":"integer","minimum":1}});
    let tool = |name, description, properties, required, read_only| {
        json!({
            "name":name,"description":description,
            "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
            "annotations":{"readOnlyHint":read_only}
        })
    };
    json!({"tools":[
        tool("start_run", "Start a managed load test: Endemic discovers the target's tools and generates tasks, then holds hold_agents agents for one stage, or without it doubles agents per stage until a stage fails or the ceiling is reached.", start, vec!["target_url","bearer"], false),
        tool("get_run", "Get the status, plan, and completed stage metrics for a run.", id.clone(), vec!["id"], true),
        tool("cancel_run", "Cancel an active run.", id, vec!["id"], false)
    ]})
}

enum CallError {
    Invalid,
    Run(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunId {
    id: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRun {
    target_url: String,
    openapi_url: Option<String>,
    bearer: Bearer,
    use_case: Option<String>,
    hold_agents: Option<usize>,
    #[serde(default)]
    pacing: Pacing,
}

fn failed((status, error): (StatusCode, String)) -> CallError {
    CallError::Run(format!(
        "Run request failed (HTTP {}): {error}",
        status.as_u16()
    ))
}

fn missing(status: StatusCode) -> CallError {
    CallError::Run(format!("Run request failed (HTTP {}).", status.as_u16()))
}

async fn call(app: App, params: &Value) -> Result<Value, CallError> {
    let name = params["name"].as_str().ok_or(CallError::Invalid)?;
    let arguments = params["arguments"]
        .as_object()
        .cloned()
        .map(Value::Object)
        .ok_or(CallError::Invalid)?;
    match name {
        "start_run" => {
            // Deserialization errors could quote the token, so they collapse to Invalid.
            let args: StartRun =
                serde_json::from_value(arguments).map_err(|_| CallError::Invalid)?;
            let request = RunRequest::new(
                &args.target_url,
                args.openapi_url.as_deref(),
                args.bearer,
                args.use_case,
                args.hold_agents,
                args.pacing,
            )
            .map_err(|error| CallError::Run(format!("Run request rejected: {error}")))?;
            start_run(app, request)
                .await
                .map(|report| json!(report))
                .map_err(failed)
        }
        "get_run" | "cancel_run" => {
            let RunId { id } = serde_json::from_value(arguments).map_err(|_| CallError::Invalid)?;
            if id == 0 {
                return Err(CallError::Invalid);
            }
            if name == "get_run" {
                get_run(&app, id)
                    .await
                    .map(|report| json!(report))
                    .map_err(missing)
            } else {
                cancel_run(&app, id)
                    .await
                    .map(|()| json!({"id":id,"cancellation_requested":true}))
                    .map_err(missing)
            }
        }
        _ => Err(CallError::Invalid),
    }
}
