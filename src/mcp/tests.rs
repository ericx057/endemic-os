use super::*;
use axum::http::Response as HttpResponse;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[tokio::test]
async fn approved_parallel_calls_overlap_keep_ids_and_preserve_model_order() {
    let ready = Arc::new(tokio::sync::Barrier::new(2));
    let target = Target::fixture("lookup", move |request| {
        let body = request_json(&request);
        match body["method"].as_str().unwrap() {
            "initialize" => Ok(initialized(None)),
            "notifications/initialized" => Ok(status(StatusCode::ACCEPTED)),
            "tools/list" => Ok(rpc_response(
                &json!(2),
                json!({"tools":[{"name":"lookup","inputSchema":{"type":"object"}}]}),
            )),
            "tools/call" => {
                let ready = ready.clone();
                let id = body["id"].as_u64().unwrap();
                let value = body["params"]["arguments"]["value"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                assert_eq!(id, if value == "first" { 3 } else { 4 });
                let bytes = futures_util::stream::once(async move {
                    ready.wait().await;
                    if id == 3 {
                        tokio::task::yield_now().await;
                    }
                    Ok::<_, std::io::Error>(
                        json!({"jsonrpc":"2.0","id":id,"result":{"content":[],"value":value}})
                            .to_string(),
                    )
                });
                Ok(HttpResponse::builder()
                    .header("content-type", "application/json")
                    .body(reqwest::Body::wrap_stream(bytes))
                    .unwrap()
                    .into())
            }
            _ => panic!("unexpected fixture method"),
        }
    });
    let metrics = crate::runner::Metrics::new(crate::runner::fixture::TRAFFIC, &["lookup".into()]);
    let mut session = target.connect().await.unwrap();
    let calls = || {
        vec![
            ("lookup".into(), json!({"value":"first"})),
            ("lookup".into(), json!({"value":"second"})),
        ]
    };
    assert_eq!(
        session.call_group(calls(), &metrics).await,
        Err(Failure::Runner)
    );
    drop(session);
    let target = target.with_parallel_tools("lookup");
    let mut session = target.connect().await.unwrap();
    session.observe(&metrics);
    let results = tokio::time::timeout(
        Duration::from_secs(1),
        session.call_group(calls(), &metrics),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(results[0]["value"], "first");
    assert_eq!(results[1]["value"], "second");
    session.close().await.unwrap();
    let report = serde_json::to_value(metrics.snapshot(1, true)).unwrap();
    assert_eq!(report["traffic"]["tool_calls"], 2);
    let sample = &report["traffic"]["session_samples"][0];
    assert_eq!(
        sample["dispatches"][0]["tools"],
        json!(["lookup", "lookup"])
    );
    assert_eq!(sample["trace_id"].as_str().unwrap().len(), 32);
    assert_eq!(report["target"]["samples"], 2);
    assert_eq!(report["hotspots"][0]["tool"], "lookup");
    assert_eq!(report["hotspots"][0]["calls"], 2);
}

#[tokio::test]
async fn every_request_carries_a_session_scoped_traceparent() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let target = Target::fixture("lookup", move |request| {
        let traceparent = request.headers()["traceparent"].to_str().unwrap();
        recorded.lock().unwrap().push(traceparent.to_owned());
        if request.method() == Method::DELETE {
            return Ok(status(StatusCode::NO_CONTENT));
        }
        let body = request_json(&request);
        Ok(match body["method"].as_str().unwrap() {
            "initialize" => initialized(Some("fixture-session")),
            "notifications/initialized" => status(StatusCode::ACCEPTED),
            "tools/list" => rpc_response(&body["id"], json!({"tools":[lookup()]})),
            "tools/call" => rpc_response(&body["id"], json!({"content":[]})),
            _ => panic!("unexpected fixture method"),
        })
    });
    let mut trace_ids = Vec::new();
    for _ in 0..2 {
        let mut session = target.connect().await.unwrap();
        session.call("lookup", json!({})).await.unwrap();
        session.close().await.unwrap();
        let parents = std::mem::take(&mut *seen.lock().unwrap());
        let ids: Vec<Vec<&str>> = parents.iter().map(|p| p.split('-').collect()).collect();
        assert_eq!(ids.len(), 5, "initialize, initialized, list, call, delete");
        assert!(ids.iter().all(|id| id[1] == ids[0][1]));
        let mut spans: Vec<_> = ids.iter().map(|id| id[2]).collect();
        spans.sort_unstable();
        spans.dedup();
        assert_eq!(spans.len(), 5);
        trace_ids.push(ids[0][1].to_owned());
    }
    assert_ne!(trace_ids[0], trace_ids[1]);
}

impl Target {
    pub(crate) fn discovering(
        respond: impl Fn(reqwest::Request) -> Result<Response, Failure> + Send + Sync + 'static,
    ) -> Self {
        let mut auth = header::HeaderValue::from_static("Bearer test-token");
        auth.set_sensitive(true);
        Self {
            fixture: Some(Box::new(respond)),
            ..Self::new(
                Url::parse("https://staging.example.com/mcp").unwrap(),
                auth,
                Wire::fixture(),
            )
        }
    }

    pub(crate) fn fixture(
        allowed: &str,
        respond: impl Fn(reqwest::Request) -> Result<Response, Failure> + Send + Sync + 'static,
    ) -> Self {
        Self {
            allowed: Some(allowed.split(',').map(str::to_owned).collect()),
            ..Self::discovering(respond)
        }
    }

    pub(crate) fn with_parallel_tools(mut self, names: &str) -> Self {
        self.parallel = names.split(',').map(str::to_owned).collect();
        self.parallel.sort_unstable();
        self
    }
}

#[test]
fn requires_correlated_successful_json_rpc_responses() {
    assert_eq!(
        decode_result(br#"{"jsonrpc":"2.0","id":1,"result":{"content":[]}}"#, 1),
        Ok(Some(json!({"content": []})))
    );
    assert_eq!(
        decode_result(br#"{"jsonrpc":"2.0","method":"notifications/test"}"#, 1),
        Ok(None)
    );
    assert_eq!(
        decode_result(br#"{"jsonrpc":"2.0","id":2,"result":{}}"#, 1),
        Err(Failure::Target)
    );
    assert_eq!(
        decode_result(br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603}}"#, 1),
        Err(Failure::Target)
    );
}

#[tokio::test]
async fn streams_a_result_and_blocks_unapproved_tools() {
    let target = Target::fixture("lookup", |request| {
        let headers = request.headers();
        assert_eq!(headers["authorization"], "Bearer test-token");
        assert!(headers["authorization"].is_sensitive());
        assert_eq!(headers["accept"], "application/json, text/event-stream");
        assert_eq!(request.url().as_str(), "https://staging.example.com/mcp");
        if request.method() == Method::DELETE {
            assert_eq!(headers["mcp-session-id"], "fixture-session");
            assert_eq!(headers["mcp-protocol-version"], PROTOCOL);
            return Ok(status(StatusCode::NO_CONTENT));
        }
        assert_eq!(request.method(), Method::POST);
        assert_eq!(headers["content-type"], "application/json");
        let body = request_json(&request);
        let id = &body["id"];
        if body["method"] != "initialize" {
            assert_eq!(headers["mcp-session-id"], "fixture-session");
            assert!(headers["mcp-session-id"].is_sensitive());
            assert_eq!(headers["mcp-protocol-version"], PROTOCOL);
        }
        let result = match body["method"].as_str().unwrap() {
            "initialize" => {
                assert_eq!(id, 1);
                assert_eq!(body["params"]["protocolVersion"], PROTOCOL);
                assert!(headers.get("mcp-session-id").is_none());
                return Ok(initialized(Some("fixture-session")));
            }
            "notifications/initialized" => {
                assert!(id.is_null());
                return Ok(status(StatusCode::ACCEPTED));
            }
            "tools/list" if body["params"]["cursor"].is_null() => {
                assert_eq!(id, 2);
                json!({
                    "tools": [{"name":"blocked","inputSchema":{"type":"object"}}],
                    "nextCursor": "second"
                })
            }
            "tools/list" => {
                assert_eq!(id, 3);
                assert_eq!(body["params"]["cursor"], "second");
                json!({"tools": [{"name":"lookup","description":"Find a fixture","inputSchema":{"type":"object"}}]})
            }
            "tools/call" => {
                assert_eq!(id, 4);
                assert_eq!(
                    body["params"],
                    json!({"name":"lookup","arguments":{"key":"example"}})
                );
                let data = json!({"jsonrpc":"2.0","id":id,"result":{"content":[]}});
                return Ok(HttpResponse::builder()
                    .header("content-type", "text/event-stream")
                    .body(format!(": comment\r\n\r\ndata: {data}\r\n\r\n"))
                    .unwrap()
                    .into());
            }
            _ => panic!("unexpected fixture method"),
        };
        Ok(rpc_response(id, result))
    });
    let mut session = target.connect().await.unwrap();
    assert_eq!(session.tools().len(), 1);
    assert_eq!(session.tools()[0].name, "lookup");
    assert_eq!(
        session.tools()[0].description.as_deref(),
        Some("Find a fixture")
    );
    assert_eq!(session.tools()[0].schema, json!({"type":"object"}));
    assert_eq!(
        session.call("blocked", json!({})).await,
        Err(Failure::Runner)
    );
    assert_eq!(
        session.call("lookup", json!([])).await,
        Err(Failure::Runner)
    );
    assert_eq!(
        session
            .call("lookup", json!({"key":"example"}))
            .await
            .unwrap(),
        json!({"content":[]})
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn rejects_bad_initialize_responses_before_discovery() {
    let cases = [
        Err(Failure::Target),
        Ok(status(StatusCode::FORBIDDEN)),
        Ok(initialized(Some(""))),
        Ok(initialized(Some("contains spaces"))),
        Ok(initialized(Some(&"x".repeat(257)))),
        Ok(rpc_response(
            &json!(1),
            json!({"protocolVersion": "unsupported", "capabilities":{"tools":{}}}),
        )),
        Ok(rpc_response(
            &json!(1),
            json!({"protocolVersion":PROTOCOL,"capabilities":{}}),
        )),
    ];
    for response in cases {
        let (target, assert_finished) = script("lookup", [("initialize", response)]);
        assert!(matches!(target.connect().await, Err(Failure::Target)));
        assert_finished();
    }
}

#[tokio::test]
async fn failed_discovery_closes_a_stateful_session() {
    for bad_response in [
        json!({"tools":false}),
        json!({"tools":[{"inputSchema":{"type":"object"}}]}),
        json!({"tools":[{"name":"lookup","inputSchema":{"type":"array"}}]}),
        json!({"tools":[lookup(), lookup()]}),
        json!({"tools":[]}),
        json!({"tools":[],"nextCursor":""}),
        json!({"tools":[],"nextCursor":"x".repeat(1025)}),
    ] {
        let (target, assert_finished) = script(
            "lookup",
            [
                ("initialize", Ok(initialized(Some("fixture-session")))),
                (
                    "notifications/initialized",
                    Ok(status(StatusCode::ACCEPTED)),
                ),
                ("tools/list", Ok(rpc_response(&json!(2), bad_response))),
                ("DELETE", Ok(status(StatusCode::NO_CONTENT))),
            ],
        );
        assert!(matches!(target.connect().await, Err(Failure::Target)));
        assert_finished();
    }
    let (target, assert_finished) = script(
        "lookup",
        [
            ("initialize", Ok(initialized(Some("fixture-session")))),
            ("notifications/initialized", Ok(status(StatusCode::OK))),
            ("DELETE", Err(Failure::Target)),
        ],
    );
    assert!(matches!(target.connect().await, Err(Failure::Target)));
    assert_finished();
}

#[tokio::test]
async fn discovery_stops_after_four_pages() {
    let target = Target::fixture("lookup", move |request| {
        let body = request_json(&request);
        assert!(request.headers().get("mcp-session-id").is_none());
        let result = match body["method"].as_str().unwrap() {
            "initialize" => initialized(None),
            "notifications/initialized" => status(StatusCode::ACCEPTED),
            "tools/list" => {
                let id = body["id"].as_u64().unwrap();
                assert!((2..6).contains(&id));
                rpc_response(&body["id"], json!({"tools":[],"nextCursor":"repeat"}))
            }
            _ => panic!("unexpected fixture method"),
        };
        Ok(result)
    });
    assert!(matches!(target.connect().await, Err(Failure::Target)));
}

#[tokio::test]
async fn local_limits_and_tool_errors_are_reported() {
    let (target, assert_finished) = script(
        "lookup",
        [
            ("initialize", Ok(initialized(None))),
            (
                "notifications/initialized",
                Ok(status(StatusCode::ACCEPTED)),
            ),
            (
                "tools/list",
                Ok(rpc_response(&json!(2), json!({"tools":[lookup()]}))),
            ),
            (
                "tools/call",
                Ok(rpc_response(
                    &json!(4),
                    json!({"content":[],"isError":true}),
                )),
            ),
            (
                "tools/call",
                Ok(rpc_response(&json!(5), json!({"content":"invalid"}))),
            ),
            ("tools/call", Err(Failure::Target)),
        ],
    );
    let mut session = target.connect().await.unwrap();
    assert_eq!(
        session
            .call(
                "lookup",
                json!({"payload":"x".repeat(Wire::fixture().request_limit)})
            )
            .await,
        Err(Failure::Runner)
    );
    for _ in 0..3 {
        assert_eq!(
            session.call("lookup", json!({})).await,
            Err(Failure::Target)
        );
    }
    session.close().await.unwrap();
    assert_finished();
}

#[tokio::test]
async fn stateful_close_tolerates_unsupported_delete_and_reports_other_failures() {
    for (status_code, expected) in [
        (StatusCode::METHOD_NOT_ALLOWED, Ok(())),
        (StatusCode::SERVICE_UNAVAILABLE, Err(Failure::Target)),
    ] {
        let (target, assert_finished) = script(
            "lookup",
            [
                ("initialize", Ok(initialized(Some("fixture-session")))),
                (
                    "notifications/initialized",
                    Ok(status(StatusCode::ACCEPTED)),
                ),
                (
                    "tools/list",
                    Ok(rpc_response(&json!(2), json!({"tools":[lookup()]}))),
                ),
                ("DELETE", Ok(status(status_code))),
            ],
        );
        assert_eq!(target.connect().await.unwrap().close().await, expected);
        assert_finished();
    }
}

#[tokio::test]
async fn discovery_allows_every_listed_tool_and_parallelizes_read_only_ones() {
    let pages = [
        json!({"tools":[
            {"name":"search","description":"Find","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}},
            {"name":"refund","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":false}}
        ],"nextCursor":"page-2"}),
        json!({"tools":[
            {"name":"lookup","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}},
            {"name":"notes","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":"true"}}
        ]}),
    ];
    let target = Target::discovering(move |request| {
        let body = request_json(&request);
        Ok(match body["method"].as_str().unwrap() {
            "initialize" => initialized(None),
            "notifications/initialized" => status(StatusCode::ACCEPTED),
            "tools/list" => {
                let page = usize::from(body["params"]["cursor"] == "page-2");
                rpc_response(&body["id"], pages[page].clone())
            }
            "tools/call" => rpc_response(&body["id"], json!({"content":[]})),
            _ => panic!("unexpected fixture method"),
        })
    });
    let (target, tools) = target.discover().await.unwrap();
    let names: Vec<_> = tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, ["search", "refund", "lookup", "notes"]);
    assert_eq!(tools[0].description.as_deref(), Some("Find"));
    let read_only: Vec<_> = tools.iter().map(|tool| tool.read_only).collect();
    assert_eq!(read_only, [true, false, true, false]);
    assert!(target.auth.is_sensitive());
    let session = target.connect().await.unwrap();
    assert_eq!(session.tools().len(), 4);
    assert!(session.can_parallel("search") && session.can_parallel("lookup"));
    assert!(!session.can_parallel("refund") && !session.can_parallel("notes"));
}

#[tokio::test]
async fn discovery_rejects_names_models_cannot_call() {
    for name in ["a/b", "", &"x".repeat(65)] {
        let tools = json!({"tools":[{"name":name,"inputSchema":{"type":"object"}}]});
        let target = Target::discovering(move |request| {
            let body = request_json(&request);
            Ok(match body["method"].as_str().unwrap() {
                "initialize" => initialized(None),
                "notifications/initialized" => status(StatusCode::ACCEPTED),
                _ => rpc_response(&body["id"], tools.clone()),
            })
        });
        assert_eq!(
            target.discover().await.err(),
            Some(Failure::Target),
            "{name}"
        );
    }
}

#[tokio::test]
async fn response_requires_a_supported_content_type_and_a_final_result() {
    for (content_type, body) in [
        ("text/plain", "{}"),
        ("application/json", "{}"),
        ("application/json", " "),
        (
            "application/json",
            r#"{"jsonrpc":"2.0","method":"notification"}"#,
        ),
        (
            "text/event-stream",
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n",
        ),
        ("text/event-stream", "data:    \n\n"),
    ] {
        let response = HttpResponse::builder()
            .header("content-type", content_type)
            .body(body)
            .unwrap()
            .into();
        assert_eq!(
            rpc_result(response, 1, Wire::fixture().response_limit).await,
            Err(Failure::Target)
        );
    }
    let missing_type = HttpResponse::builder().body("{}").unwrap().into();
    assert_eq!(
        rpc_result(missing_type, 1, Wire::fixture().response_limit).await,
        Err(Failure::Target)
    );
}

#[tokio::test]
async fn oversized_responses_fail() {
    let response = axum::http::Response::builder()
        .header("content-type", "text/event-stream")
        .body(format!(
            "data: {}\n\n",
            "x".repeat(Wire::fixture().response_limit)
        ))
        .unwrap();
    assert_eq!(
        rpc_result(response.into(), 1, Wire::fixture().response_limit).await,
        Err(Failure::Target)
    );
}

#[tokio::test]
async fn sse_decoder_skips_notifications_and_returns_the_matching_result() {
    let response = axum::http::Response::builder()
        .header("content-type", "text/event-stream; charset=utf-8")
        .body(concat!(
            ": comment\r\n\r\n",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\n",
            "data: {\"jsonrpc\":\"2.0\",\n",
            "data: \"id\":42,\"result\":{\"content\":[]}}\n\n"
        ))
        .unwrap();
    assert_eq!(
        rpc_result(response.into(), 42, Wire::fixture().response_limit).await,
        Ok(json!({"content":[]}))
    );
}

fn request_json(request: &reqwest::Request) -> Value {
    serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap()
}

fn status(code: StatusCode) -> Response {
    HttpResponse::builder()
        .status(code)
        .body("")
        .unwrap()
        .into()
}

fn lookup() -> Value {
    json!({"name":"lookup","inputSchema":{"type":"object"}})
}

fn rpc_response(id: &Value, result: Value) -> Response {
    HttpResponse::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .body(json!({"jsonrpc":"2.0","id":id,"result":result}).to_string())
        .unwrap()
        .into()
}

fn initialized(session: Option<&str>) -> Response {
    let mut response = rpc_response(
        &json!(1),
        json!({"protocolVersion":PROTOCOL,"capabilities":{"tools":{}}}),
    );
    if let Some(session) = session {
        response
            .headers_mut()
            .insert("mcp-session-id", session.parse().unwrap());
    }
    response
}

fn script<const N: usize>(
    allowed: &str,
    steps: [(&'static str, Result<Response, Failure>); N],
) -> (Target, impl FnOnce()) {
    let steps = Arc::new(Mutex::new(steps.into_iter()));
    let pending = steps.clone();
    let target = Target::fixture(allowed, move |request| {
        let (expected, response) = pending.lock().unwrap().next().expect("unexpected request");
        if expected == "DELETE" {
            assert_eq!(request.method(), Method::DELETE);
            assert_eq!(request.headers()["mcp-session-id"], "fixture-session");
        } else {
            assert_eq!(request.method(), Method::POST);
            assert_eq!(request_json(&request)["method"], expected);
        }
        response
    });
    (target, move || assert_eq!(steps.lock().unwrap().len(), 0))
}
