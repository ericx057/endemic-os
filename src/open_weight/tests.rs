use super::*;
use crate::config::tests::{assert_each_required, env_of};
use axum::http::Response as HttpResponse;
use reqwest::{Method, Request, Response};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering::Relaxed},
};

const BUDGETS: [(&str, &str); 3] = [
    ("AGENT_MAX_MODEL_TURNS", "8"),
    ("AGENT_MAX_TOOL_CALLS", "8"),
    ("AGENT_RESULT_LIMIT_BYTES", "65536"),
];

fn faults() -> SessionFaults {
    SessionFaults::new(0, 0, std::time::Duration::ZERO)
}

fn budget() -> Budget {
    Budget { turns: 8, calls: 8 }
}

fn settings() -> Vec<(&'static str, &'static str)> {
    [client::tests::SETTINGS.as_slice(), &BUDGETS].concat()
}

impl Engine {
    pub(super) fn fixture() -> Self {
        Self::from_env(&env_of(&settings())).unwrap()
    }

    pub(crate) fn scripted(
        respond: impl Fn(Request) -> Result<Response, Failure> + Send + Sync + 'static,
    ) -> Self {
        Self {
            client: Client::fixture(respond),
            ..Self::fixture()
        }
    }
}

#[test]
fn every_engine_setting_is_required() {
    assert_each_required(&settings(), Engine::from_env);
}

#[tokio::test]
async fn turns_append_verbatim_calls_and_their_whole_results() {
    let mcp_requests = Arc::new(Mutex::new(Vec::new()));
    let target = Target::Mcp(target_fixture(mcp_requests.clone()));
    let turns = Arc::new(AtomicUsize::new(0));
    let observed = turns.clone();
    let first_call = raw_call("first", "lookup", "{ \"query\" : \"initial\" }");
    let engine = Engine {
        client: Client::fixture(move |request| {
            let request = request_json(&request);
            assert_eq!(
                request["messages"][1],
                json!({"role":"user","content":"Find and verify the result marker."})
            );
            Ok(match observed.fetch_add(1, Relaxed) {
                0 => calls(vec![first_call.clone()]),
                1 => {
                    assert_eq!(
                        request["messages"][2],
                        json!({"role":"assistant","content":null,"tool_calls":[first_call.clone()]})
                    );
                    assert_eq!(request["messages"][3]["tool_call_id"], "first");
                    let content = tool_content(&request, 3);
                    assert_eq!(content["remaining_tool_calls"], 7);
                    assert!(content["result"].to_string().contains("TAIL_MARKER"));
                    calls(vec![call(
                        "verify",
                        "lookup",
                        json!({"query":"TAIL_MARKER"}),
                    )])
                }
                2 => {
                    let content = tool_content(&request, 5);
                    assert_eq!(content["remaining_tool_calls"], 6);
                    assert!(content["result"].to_string().contains("verified"));
                    done()
                }
                _ => panic!("unexpected model turn"),
            })
        }),
        ..Engine::fixture()
    };
    let metrics = Metrics::default();
    assert_eq!(
        engine
            .session(
                &target,
                &lookup_tools(),
                "Find and verify the result marker.",
                budget(),
                &metrics,
                faults(),
            )
            .await,
        Ok(())
    );
    assert_eq!(
        *mcp_requests.lock().unwrap(),
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/call",
            "tools/call",
            "DELETE"
        ]
    );
    assert_eq!(turns.load(Relaxed), 3);
}

#[tokio::test]
async fn a_result_over_the_limit_fails_the_session_as_a_target_error() {
    let target = Target::Mcp(target_fixture(Arc::default()));
    let engine = Engine {
        client: Client::fixture(|_| {
            Ok(calls(vec![call("a", "lookup", json!({"query":"initial"}))]))
        }),
        result_limit: 100,
        ..Engine::fixture()
    };
    assert_eq!(
        engine
            .session(
                &target,
                &lookup_tools(),
                "Find facts.",
                budget(),
                &Metrics::default(),
                faults()
            )
            .await,
        Err(Failure::Target)
    );
}

#[tokio::test]
async fn rejects_entire_turn_with_unknown_tool_and_closes_target() {
    let mcp_requests = Arc::new(Mutex::new(Vec::new()));
    let target = Target::Mcp(target_fixture(mcp_requests.clone()));
    let metrics = Metrics::default();
    let engine = Engine {
        client: Client::fixture(|_| {
            Ok(calls(vec![
                call("permitted", "lookup", json!({"query":"initial"})),
                call("unknown", "not_approved", json!({})),
            ]))
        }),
        ..Engine::fixture()
    };
    assert_eq!(
        engine
            .session(
                &target,
                &lookup_tools(),
                "Find facts.",
                budget(),
                &metrics,
                faults()
            )
            .await,
        Err(Failure::Inference)
    );
    assert_eq!(
        *mcp_requests.lock().unwrap(),
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "DELETE"
        ]
    );
}

#[tokio::test]
async fn model_batch_keeps_response_order_for_operator_approved_tools() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let target = Target::Mcp(target_fixture(requests).with_parallel_tools("lookup"));
    let turn = AtomicUsize::new(0);
    let engine = Engine {
        client: Client::fixture(move |request| {
            let request = request_json(&request);
            match turn.fetch_add(1, Relaxed) {
                0 => Ok(calls(vec![
                    call("first", "lookup", json!({"query":"initial"})),
                    call("second", "lookup", json!({"query":"TAIL_MARKER"})),
                ])),
                1 => {
                    assert_eq!(request["messages"][3]["tool_call_id"], "first");
                    assert!(
                        tool_content(&request, 3)
                            .to_string()
                            .contains("TAIL_MARKER")
                    );
                    assert_eq!(request["messages"][4]["tool_call_id"], "second");
                    assert!(tool_content(&request, 4).to_string().contains("verified"));
                    Ok(done())
                }
                _ => panic!("unexpected model turn"),
            }
        }),
        ..Engine::fixture()
    };
    let metrics = Metrics::default();
    assert_eq!(
        engine
            .session(
                &target,
                &lookup_tools(),
                "Read both facts.",
                budget(),
                &metrics,
                faults()
            )
            .await,
        Ok(())
    );
    let report = serde_json::to_value(metrics.snapshot(1, true)).unwrap();
    assert_eq!(report["traffic"]["tool_calls"], 2);
    assert_eq!(
        report["traffic"]["session_samples"][0]["dispatches"][0]["tools"],
        json!(["lookup", "lookup"])
    );
}

#[tokio::test]
async fn drives_an_http_api_target_unchanged() {
    let (addr, seen) = crate::http_api::tests::serve().await;
    let target = crate::http_api::Target::loopback(addr);
    let (tools, labels) = (model_tools(target.tools()).unwrap(), target.labels());
    let target = Target::Http(target);
    let bodies = Arc::new(Mutex::new(Vec::<Value>::new()));
    let observed = bodies.clone();
    let engine = Engine {
        client: Client::fixture(move |request| {
            let mut bodies = observed.lock().unwrap();
            bodies.push(request_json(&request));
            Ok(match bodies.len() {
                1 => calls(vec![call("call-7", "getItem", json!({"path":{"id":"7"}}))]),
                _ => done(),
            })
        }),
        ..Engine::fixture()
    };
    let metrics = Metrics::new(crate::runner::fixture::TRAFFIC, &labels);
    assert_eq!(
        engine
            .session(
                &target,
                &tools,
                "fetch item 7",
                budget(),
                &metrics,
                faults()
            )
            .await,
        Ok(())
    );
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    let tools = bodies[0]["tools"].as_array().unwrap();
    let tool = tools
        .iter()
        .map(|tool| &tool["function"])
        .find(|tool| tool["name"] == "getItem")
        .unwrap();
    assert_eq!(tool["parameters"]["required"], json!(["path"]));
    assert_eq!(bodies[1]["messages"][3]["tool_call_id"], "call-7");
    assert!(tool_content(&bodies[1], 3).to_string().contains("found"));
    assert_eq!(seen.lock().unwrap()[0].uri, "/api/items/7");
    let report = serde_json::to_value(metrics.snapshot(1, true)).unwrap();
    assert_eq!(report["hotspots"][0]["tool"], "getItem (GET /items/{id})");
}

fn lookup_tools() -> Arc<RawValue> {
    let lookup = Tool {
        name: "lookup".into(),
        description: None,
        schema: json!({"type":"object"}),
        read_only: false,
    };
    model_tools(&[lookup]).unwrap()
}

fn target_fixture(requests: Arc<Mutex<Vec<String>>>) -> crate::mcp::Target {
    crate::mcp::Target::fixture("lookup", move |request| {
        if request.method() == Method::DELETE {
            requests.lock().unwrap().push("DELETE".into());
            return Ok(HttpResponse::builder()
                .status(204)
                .body(String::new())
                .unwrap()
                .into());
        }
        let body = request_json(&request);
        let method = body["method"].as_str().unwrap();
        requests.lock().unwrap().push(method.to_owned());
        let result = match method {
            "initialize" => json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}}}),
            "notifications/initialized" => {
                return Ok(HttpResponse::builder()
                    .status(202)
                    .body(String::new())
                    .unwrap()
                    .into());
            }
            "tools/list" => json!({"tools":[{"name":"lookup","inputSchema":{"type":"object"}}]}),
            "tools/call" => match body["params"]["arguments"]["query"].as_str().unwrap() {
                "initial" => {
                    json!({"content":[{"type":"text","text": format!("{}TAIL_MARKER", "x".repeat(900))}]})
                }
                "TAIL_MARKER" => json!({"content":[{"type":"text","text":"verified"}]}),
                _ => panic!("unexpected tool arguments"),
            },
            _ => panic!("unexpected MCP method"),
        };
        let mut response = HttpResponse::builder().header("content-type", "application/json");
        if method == "initialize" {
            response = response.header("mcp-session-id", "open-weight-session");
        }
        Ok(response
            .body(json!({"jsonrpc":"2.0","id":body["id"],"result":result}).to_string())
            .unwrap()
            .into())
    })
}

fn body_text(request: &Request) -> String {
    String::from_utf8(request.body().unwrap().as_bytes().unwrap().to_vec()).unwrap()
}

fn request_json(request: &Request) -> Value {
    serde_json::from_str(&body_text(request)).unwrap()
}

fn tool_content(request: &Value, index: usize) -> Value {
    assert_eq!(request["messages"][index]["role"], "tool");
    serde_json::from_str(request["messages"][index]["content"].as_str().unwrap()).unwrap()
}

fn calls(tool_calls: Vec<Value>) -> Response {
    inference(
        "tool_calls",
        json!({"role":"assistant","content":null,"tool_calls":tool_calls}),
    )
}

fn done() -> Response {
    inference("stop", json!({"role":"assistant","content":"Verified."}))
}

fn inference(finish_reason: &str, message: Value) -> Response {
    HttpResponse::builder()
        .header("content-type", "application/json")
        .body(
            json!({
                "choices":[{"finish_reason":finish_reason,"message":message}],
                "usage":{"prompt_tokens":60,"completion_tokens":10}
            })
            .to_string(),
        )
        .unwrap()
        .into()
}

fn call(id: &str, name: &str, arguments: Value) -> Value {
    raw_call(id, name, &arguments.to_string())
}

fn raw_call(id: &str, name: &str, arguments: &str) -> Value {
    json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments}})
}
