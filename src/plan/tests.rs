use super::*;
use crate::{
    config::tests::{assert_each_required, env_of},
    request::tests::FIXTURE_TOKEN,
};
use axum::{Json, Router, http::HeaderMap, response::IntoResponse, routing::get};
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

/// Fixture planning settings; production values come from the deployment's runner config.
const SETTINGS: [(&str, &str); 7] = [
    ("TARGET_CONNECT_TIMEOUT_SECONDS", "3"),
    ("TARGET_TIMEOUT_SECONDS", "10"),
    ("TARGET_REQUEST_LIMIT_BYTES", "16384"),
    ("TARGET_RESPONSE_LIMIT_BYTES", "65536"),
    ("OPENAPI_SPEC_LIMIT_BYTES", "4096"),
    ("RUN_MAX_TOOLS", "3"),
    ("PLAN_TASK_COUNT", "2"),
];
pub(crate) const TASKS: [&str; 2] = ["Search for order 1.", "Refund order 2."];

impl Planner {
    /// A planner that may reach loopback fixtures; production egress refuses them.
    pub(crate) fn loopback(engine: Engine) -> Self {
        Self {
            egress: Egress::loopback(),
            ..Self::from_env(&env_of(&SETTINGS), Arc::new(engine)).unwrap()
        }
    }
}

#[test]
fn every_planning_setting_is_required() {
    assert_each_required(&SETTINGS, |env| {
        Planner::from_env(env, Arc::new(Engine::scripted(|_| unreachable!())))
    });
}

fn completion(finish: &str, message: Value) -> reqwest::Response {
    let body = json!({
        "choices":[{"finish_reason":finish,"message":message}],
        "usage":{"prompt_tokens":10,"completion_tokens":5}
    });
    axum::http::Response::builder()
        .header("content-type", "application/json")
        .body(body.to_string())
        .unwrap()
        .into()
}

/// A stand-in vLLM: the planning call gets `TASKS`; an agent calls `search` once, then stops.
/// Every request body is kept.
pub(crate) fn inference(
    seen: Arc<Mutex<Vec<Value>>>,
) -> impl Fn(reqwest::Request) -> Result<reqwest::Response, Failure> + Send + Sync + 'static {
    move |request| {
        let body: Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        seen.lock().unwrap().push(body.clone());
        let message = |content: &str| json!({"role":"assistant","content":content});
        if body.get("response_format").is_some() {
            return Ok(completion("stop", message(&json!(TASKS).to_string())));
        }
        let messages = body["messages"].as_array().unwrap();
        if messages.last().unwrap()["role"] == "user" {
            let call = json!({"id":"c1","type":"function",
                "function":{"name":"search","arguments":"{}"}});
            let message = json!({"role":"assistant","content":null,"tool_calls":[call]});
            return Ok(completion("tool_calls", message));
        }
        Ok(completion("stop", message("Done.")))
    }
}

/// A stand-in MCP server over loopback HTTP, listing `tools` and recording each
/// request's method and Authorization header.
pub(crate) async fn mcp_server(tools: Value) -> (SocketAddr, Arc<Mutex<Vec<(String, String)>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let app = Router::new().route(
        "/mcp",
        axum::routing::post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let (recorded, tools) = (recorded.clone(), tools.clone());
            async move {
                let method = body["method"].as_str().unwrap_or_default().to_owned();
                let auth = headers["authorization"].to_str().unwrap().to_owned();
                recorded.lock().unwrap().push((method.clone(), auth));
                let result = match method.as_str() {
                    "notifications/initialized" => {
                        return axum::http::StatusCode::ACCEPTED.into_response();
                    }
                    "initialize" => json!({"protocolVersion":"2025-11-25",
                        "capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
                    "tools/list" => json!({"tools":tools}),
                    _ => json!({"content":[{"type":"text","text":"ok"}]}),
                };
                Json(json!({"jsonrpc":"2.0","id":body["id"],"result":result})).into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, seen)
}

pub(crate) fn mcp_tools() -> Value {
    json!([
        {"name":"search","description":"Find orders","inputSchema":{"type":"object"},
            "annotations":{"readOnlyHint":true}},
        {"name":"refund","inputSchema":{"type":"object"}}
    ])
}

#[tokio::test]
async fn mcp_run_is_planned_from_discovery_and_generated_tasks() {
    let (addr, seen) = mcp_server(mcp_tools()).await;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let planner = Planner::loopback(Engine::scripted(inference(requests.clone())));
    let request = RunRequest::loopback(&format!("http://{addr}/mcp"), None, Some("Shoppers."));
    let Prepared {
        plan,
        labels,
        sessions,
    } = planner.prepare(request).await.ok().unwrap();
    assert_eq!(labels, ["search", "refund"]);
    let plan = serde_json::to_value(plan).unwrap();
    assert_eq!(
        plan,
        json!({"target_kind":"mcp","tools":["search","refund"],"parallel_tools":["search"],
            "tasks":TASKS,"hold_agents":null,"pacing":{"start_spread_ms":0,"think_min_ms":0,"think_max_ms":0,"sync_sessions":false},"faults":[],"agents_per_stage":[]})
    );
    let planning = requests.lock().unwrap()[0].clone();
    assert!(
        planning["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("Shoppers.")
    );
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|(_, auth)| auth == &format!("Bearer {FIXTURE_TOKEN}"))
    );

    // Round-robin: agents 0, 1, 2 pursue tasks 0, 1, 0.
    let metrics = Arc::new(Metrics::default());
    for agent in 0..3 {
        assert_eq!(sessions(agent, metrics.clone()).await, Ok(()));
    }
    let objectives: Vec<_> = requests
        .lock()
        .unwrap()
        .iter()
        .filter(|body| body["messages"].as_array().unwrap().len() == 2)
        .skip(1)
        .map(|body| body["messages"][1]["content"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(objectives, [TASKS[0], TASKS[1], TASKS[0]]);
}

/// A loopback HTTP API serving its own OpenAPI document at /openapi.json.
async fn http_api(spec: Value) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let auth = Arc::new(Mutex::new(Vec::new()));
    let recorded = auth.clone();
    let app = Router::new().route(
        "/openapi.json",
        get(move |headers: HeaderMap| {
            let header = headers.get("authorization").map(|v| v.to_str().unwrap());
            recorded
                .lock()
                .unwrap()
                .push(header.unwrap_or_default().to_owned());
            let spec = spec.clone();
            async move { Json(spec) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, auth)
}

#[tokio::test]
async fn http_run_turns_every_operation_into_a_tool() {
    let (addr, auth) = http_api(crate::http_api::tests::spec()).await;
    let planner = Planner::loopback(Engine::scripted(inference(Arc::default())));
    let base = format!("http://{addr}/api");
    let spec = format!("http://{addr}/openapi.json");
    let request = RunRequest::loopback(&base, Some(&spec), None);
    // The fixture spec has four operations; RUN_MAX_TOOLS is three.
    let Failed { plan, error } = planner.prepare(request).await.err().unwrap();
    assert_eq!(
        error,
        "the target exposes 4 tools; a run needs 1 to 3 (RUN_MAX_TOOLS)"
    );
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        json!({"target_kind":"http","tools":["fail","createItem","getItem","large"],
            "parallel_tools":["fail","getItem","large"],"tasks":[],"hold_agents":null,"pacing":{"start_spread_ms":0,"think_min_ms":0,"think_max_ms":0,"sync_sessions":false},"faults":[],"agents_per_stage":[]})
    );
    assert_eq!(*auth.lock().unwrap(), [format!("Bearer {FIXTURE_TOKEN}")]);

    let planner = Planner {
        max_tools: 4,
        ..Planner::loopback(Engine::scripted(inference(Arc::default())))
    };
    let request = RunRequest::loopback(&base, Some(&spec), None);
    let prepared = planner.prepare(request).await.ok().unwrap();
    assert_eq!(prepared.plan.tasks, TASKS);
}

#[tokio::test]
async fn spec_fetch_is_bounded_and_must_be_openapi_json() {
    let big = json!({"openapi":"3.1.0","paths":{},"pad":"x".repeat(5000)});
    for (spec, expected) in [
        (big, "exceeds OPENAPI_SPEC_LIMIT_BYTES (4096 bytes)"),
        (json!({"swagger":"2.0"}), "OpenAPI 3"),
    ] {
        let (addr, _) = http_api(spec).await;
        let planner = Planner::loopback(Engine::scripted(|_| unreachable!()));
        let spec = format!("http://{addr}/openapi.json");
        let request = RunRequest::loopback(&format!("http://{addr}/"), Some(&spec), None);
        let error = planner.prepare(request).await.err().unwrap().error;
        assert!(error.contains(expected), "{error}");
    }
}

#[tokio::test]
async fn failures_before_stages_keep_what_was_decided() {
    // SSRF guard: production egress refuses the loopback fixture before any request.
    let planner = Planner {
        egress: Egress::new(),
        ..Planner::loopback(Engine::scripted(|_| unreachable!()))
    };
    let request = RunRequest::loopback("https://127.0.0.1/mcp", None, None);
    let Failed { plan, error } = planner.prepare(request).await.err().unwrap();
    assert_eq!(error, "127.0.0.1 resolves to a non-public address");
    assert!(plan.tools.is_empty() && plan.tasks.is_empty());

    // Task generation failure: tools are known, tasks are not.
    let (addr, _) = mcp_server(mcp_tools()).await;
    let planner = Planner::loopback(Engine::scripted(|_| Err(Failure::Inference)));
    let request = RunRequest::loopback(&format!("http://{addr}/mcp"), None, None);
    let Failed { plan, error } = planner.prepare(request).await.err().unwrap();
    assert!(error.starts_with("task generation failed"), "{error}");
    assert_eq!(plan.tools, ["search", "refund"]);
    assert!(plan.tasks.is_empty());

    // Discovery failure: nothing listens on the fixture's port any more.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = closed.local_addr().unwrap();
    drop(closed);
    let request = RunRequest::loopback(&format!("http://{addr}/mcp"), None, None);
    let error = planner.prepare(request).await.err().unwrap().error;
    assert!(error.starts_with("MCP tool discovery failed"), "{error}");
}

#[test]
fn without_a_mix_agents_cycle_through_tasks() {
    let group = Group {
        name: String::new(),
        weight: 100,
        max_model_turns: 1,
        max_tool_calls: 1,
        tasks: 0..3,
    };
    let mix = TaskMix::new(vec![group], 3).unwrap();
    let picked: Vec<_> = (0..205).map(|agent| mix.assign(agent).1).collect();
    assert!(
        picked
            .iter()
            .enumerate()
            .all(|(agent, task)| *task == agent % 3)
    );
}

#[tokio::test]
async fn injected_errors_never_reach_the_target_and_unknown_tools_are_refused() {
    let (addr, seen) = mcp_server(mcp_tools()).await;
    let url = format!("http://{addr}/mcp");
    let planner = Planner::loopback(Engine::scripted(inference(Arc::default())));
    let fault: Fault = serde_json::from_value(json!({"tool":"search","first_stage":1,
        "last_stage":1,"percent":100,"effect":{"kind":"error"}}))
    .unwrap();
    let request = |fault: Fault| RunRequest {
        faults: vec![fault],
        ..RunRequest::loopback(&url, None, None)
    };
    let unknown = Fault {
        tool: "delete".into(),
        ..fault.clone()
    };
    let error = planner.prepare(request(unknown)).await.err().unwrap().error;
    assert_eq!(
        error,
        r#"fault tool "delete" is not one of the target's tools"#
    );

    let prepared = planner.prepare(request(fault.clone())).await.ok().unwrap();
    assert_eq!(prepared.plan.faults, std::slice::from_ref(&fault));
    let stage = crate::runner::StageFaults::new(1, 1, &prepared.plan.faults);
    let metrics = Arc::new(Metrics::default().with_faults(stage));
    seen.lock().unwrap().clear();
    let result = (prepared.sessions)(0, metrics.clone()).await;
    assert_eq!(result, Err(Failure::Injected));
    metrics.finished(result);
    let methods: Vec<_> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|(method, _)| method.clone())
        .collect();
    assert!(
        !methods.iter().any(|method| method == "tools/call"),
        "{methods:?}"
    );
    let report = serde_json::to_value(metrics.snapshot(1, true)).unwrap();
    assert_eq!(report["injected"]["search"]["errors"], 1);
    assert_eq!(
        (
            report["injected_failures"].clone(),
            report["target_errors"].clone()
        ),
        (json!(1), json!(0))
    );
}

#[tokio::test]
async fn a_task_mix_generates_tasks_per_persona_and_checks_budgets() {
    let (addr, _) = mcp_server(mcp_tools()).await;
    let url = format!("http://{addr}/mcp");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let planner = Planner::loopback(Engine::scripted(inference(requests.clone())));
    let request = |mix: Value| RunRequest {
        task_mix: serde_json::from_value(mix).unwrap(),
        ..RunRequest::loopback(&url, None, None)
    };
    let mix = json!([{"name":"reads","weight":70,"use_case":"Browse."},
        {"name":"writes","weight":30,"max_tool_calls":4}]);
    let prepared = planner.prepare(request(mix)).await.ok().unwrap();
    assert_eq!(prepared.plan.tasks, [TASKS, TASKS].concat());
    assert_eq!(
        serde_json::to_value(&prepared.plan.task_mix).unwrap(),
        json!([{"name":"reads","weight":70,"max_model_turns":8,"max_tool_calls":8,"tasks":{"start":0,"end":2}},
            {"name":"writes","weight":30,"max_model_turns":8,"max_tool_calls":4,"tasks":{"start":2,"end":4}}])
    );
    let prompts: Vec<String> = requests
        .lock()
        .unwrap()
        .iter()
        .map(|body| body["messages"][1]["content"].to_string())
        .collect();
    assert!(prompts[0].contains("Browse.") && !prompts[1].contains("Browse."));

    let over = json!([{"name":"reads","weight":100,"max_tool_calls":9}]);
    let error = planner.prepare(request(over)).await.err().unwrap().error;
    assert_eq!(
        error,
        "persona reads max_tool_calls must be from 1 to 8 (AGENT_MAX_TOOL_CALLS)"
    );
}
