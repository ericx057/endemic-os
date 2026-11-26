use super::*;
use serde_json::{Value, json};

async fn rpc(app: &Router, method: &str, params: Value) -> (StatusCode, Value) {
    exchange(
        app,
        json!({"jsonrpc":"2.0","id":5,"method":method,"params":params}),
        None,
    )
    .await
}

async fn exchange(app: &Router, message: Value, version: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header("authorization", "Bearer api-secret");
    if let Some(version) = version {
        request = request.header("mcp-protocol-version", version);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(message.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 16_384)
        .await
        .unwrap();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

#[tokio::test]
async fn legacy_lifecycle_negotiates_version_and_handles_notifications() {
    let app = router(App::fixture(Settings::fixture(), |_, _| async { Ok(()) }));
    for (requested, expected) in [("2025-06-18", "2025-06-18"), ("2099-01-01", "2025-11-25")] {
        let (status, body) = rpc(&app, "initialize", json!({"protocolVersion":requested})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["protocolVersion"], expected);
        assert_eq!(body["result"]["serverInfo"]["name"], "endemic");
    }
    let (_, body) = rpc(&app, "initialize", json!({})).await;
    assert_eq!(body["error"]["code"], -32602);
    let (status, body) = exchange(
        &app,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        None,
    )
    .await;
    assert_eq!((status, body), (StatusCode::ACCEPTED, Value::Null));
    let (status, _) = exchange(&app, json!({"jsonrpc":"2.0","method":"tools/call"}), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, body) = rpc(&app, "ping", json!({})).await;
    assert_eq!(body["result"], json!({}));
    let (status, body) = rpc(&app, "missing", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], -32601);
}

#[tokio::test]
async fn malformed_rpc_and_unsupported_protocol_fail_before_dispatch() {
    let app = router(App::fixture(Settings::fixture(), |_, _| async { Ok(()) }));
    for invalid in [
        json!({}),
        json!({"jsonrpc":"2.0","id":false,"method":"ping"}),
    ] {
        let (status, body) = exchange(&app, invalid, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], -32600);
    }
    let (status, body) = exchange(
        &app,
        json!({"jsonrpc":"2.0","id":5,"method":"ping"}),
        Some("2099-01-01"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], -32022);
    assert_eq!(body["error"]["data"]["requested"], "2099-01-01");
    let response = app
        .oneshot(
            Request::post("/mcp")
                .header("origin", "https://unapproved.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn mcp_start_report_and_cancel_respect_run_scope() {
    let app = router(App::fixture(Settings::fixture(), |_, _| {
        std::future::pending::<Result<(), Failure>>()
    }));
    let call = |name: &str, arguments: Value| json!({"name":name,"arguments":arguments});
    let (_, body) = rpc(
        &app,
        "tools/call",
        call(
            "start_run",
            json!({"target_url":"https://staging.example.com/api",
                "openapi_url":"https://staging.example.com/openapi.json",
                "bearer":"staging-token","use_case":"Shoppers check out."}),
        ),
    )
    .await;
    let id = body["result"]["structuredContent"]["id"].as_u64().unwrap();
    let (_, body) = rpc(&app, "tools/call", call("get_run", json!({"id":id}))).await;
    assert_eq!(body["result"]["structuredContent"]["status"], "running");
    assert_eq!(
        body["result"]["structuredContent"]["plan"]["target_kind"],
        "http"
    );
    let (_, body) = rpc(
        &app,
        "tools/call",
        call(
            "start_run",
            json!({"target_url":"https://staging.example.com/mcp","bearer":"staging-token"}),
        ),
    )
    .await;
    assert_eq!(body["result"]["isError"], true);
    assert!(
        body["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("Run request failed (HTTP 409): a run is already in progress")
    );
    let (_, body) = rpc(&app, "tools/call", call("get_run", json!({"id":id + 1}))).await;
    assert_eq!(body["result"]["isError"], true);
    assert!(
        body["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("404")
    );
    let (_, body) = rpc(&app, "tools/call", call("cancel_run", json!({"id":id}))).await;
    assert_eq!(
        body["result"]["structuredContent"]["cancellation_requested"],
        true
    );

    for invalid in [
        call("invalid", json!({})),
        call("cancel_run", json!({"id":0})),
        call("start_fixed_run", json!({"agents":1,"hold_seconds":60})),
        call(
            "start_run",
            json!({"target_url":"https://s.example.com","bearer":"t","agents":1}),
        ),
        call("start_run", json!({"target_url":"https://s.example.com"})),
        call(
            "start_run",
            json!({"target_url":"https://s.example.com","bearer":7}),
        ),
        call("get_run", Value::Null),
    ] {
        let (_, body) = rpc(&app, "tools/call", invalid).await;
        assert_eq!(body["error"]["code"], -32602);
    }
}

#[tokio::test]
async fn start_run_advertises_the_managed_contract_and_validates_like_post_runs() {
    let app = router(App::fixture(Settings::fixture(), |_, _| async { Ok(()) }));
    let (_, body) = rpc(&app, "tools/list", json!({})).await;
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<_> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["start_run", "get_run", "cancel_run"]);
    let schema = &tools[0]["inputSchema"];
    assert_eq!(schema["required"], json!(["target_url", "bearer"]));
    let mut properties: Vec<_> = schema["properties"].as_object().unwrap().keys().collect();
    properties.sort_unstable();
    assert_eq!(
        properties,
        [
            "bearer",
            "faults",
            "hold_agents",
            "openapi_url",
            "pacing",
            "target_url",
            "task_mix",
            "use_case"
        ]
    );

    let start = json!({"name":"start_run","arguments":{
        "target_url":"http://s.example.com","bearer":"staging-token"}});
    let (_, body) = rpc(&app, "tools/call", start).await;
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("Run request rejected: target.url must be an HTTPS URL"));
    assert!(!body.to_string().contains("staging-token"));
}
#[tokio::test]
async fn mcp_modern_discovery_and_mismatched_headers() {
    let app = router(App::fixture(Settings::fixture(), |_, _| async { Ok(()) }));
    let request = |method: &str| {
        Request::post("/mcp")
        .header("authorization", "Bearer api-secret")
        .header("content-type", "application/json")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", method)
        .body(Body::from(r#"{"jsonrpc":"2.0","id":7,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#)).unwrap()
    };
    let response = app
        .clone()
        .oneshot(request("server/discover"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 16_384)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["result"]["resultType"], "complete");
    assert_eq!(body["result"]["supportedVersions"][0], "2026-07-28");
    let response = app.oneshot(request("tools/call")).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mcp_discovers_tools_and_shares_run_state_with_http_api() {
    let app = router(App::fixture(Settings::fixture(), |_, _| {
        std::future::pending::<Result<(), Failure>>()
    }));
    let request = |message: serde_json::Value, bearer: bool| {
        let mut request = Request::post("/mcp").header("content-type", "application/json");
        if bearer {
            request = request.header("authorization", "Bearer api-secret");
        }
        request.body(Body::from(message.to_string())).unwrap()
    };
    let list = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    assert_eq!(
        app.clone()
            .oneshot(request(list.clone(), false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = app.clone().oneshot(request(list, true)).await.unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), 16_384)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["result"]["tools"].as_array().unwrap().len(), 3);

    let response = app
        .clone()
        .oneshot(request(
            serde_json::json!({
                "jsonrpc":"2.0", "id":"start", "method":"tools/call", "params":{
                    "name":"start_run", "arguments":{
                        "target_url":"https://staging.example.com/mcp", "bearer":"staging-token"
                    }
                }
            }),
            true,
        ))
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), 16_384)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["id"], "start");
    assert_eq!(body["result"]["structuredContent"]["id"], 1);

    let report = Request::get("/runs/1")
        .header("authorization", "Bearer api-secret")
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(report).await.unwrap().status(), StatusCode::OK);
}
