use super::*;
use crate::runner::fixture::TRAFFIC;
use axum::{body::to_bytes, extract::Request, http::StatusCode as AxumStatus};
use reqwest::Method;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

const BASE: &str = "https://staging.example.com/api";

pub(crate) fn spec() -> Value {
    json!({"openapi": "3.1.0", "paths": {
        "/items/{id}": {"get": {"operationId": "getItem", "summary": "Fetch one item",
            "parameters": [
                {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}},
                {"name": "limit", "in": "query", "schema": {"type": "integer"}}
            ]}},
        "/items": {"post": {"operationId": "createItem",
            "requestBody": {"content": {"application/json": {"schema": {"type": "object"}}}}}},
        "/fail": {"get": {"operationId": "fail"}},
        "/large": {"get": {"operationId": "large"}}
    }})
}

fn auth() -> header::HeaderValue {
    let mut auth = header::HeaderValue::from_static("Bearer test-token");
    auth.set_sensitive(true);
    auth
}

impl Target {
    /// Production URLs must be HTTPS; tests swap in a plain-HTTP loopback server.
    pub(crate) fn loopback(addr: SocketAddr) -> Self {
        let url = Url::parse(&format!("http://{addr}/api")).unwrap();
        Self::new(url, auth(), &spec(), Wire::fixture()).unwrap()
    }
}

fn target() -> Target {
    Target::new(Url::parse(BASE).unwrap(), auth(), &spec(), Wire::fixture()).unwrap()
}

#[derive(Debug)]
pub(crate) struct Seen {
    pub(crate) method: String,
    pub(crate) uri: String,
    pub(crate) authorization: String,
    pub(crate) accept: String,
    pub(crate) traceparent: String,
    pub(crate) body: String,
}

/// An axum stand-in for the staging API that records every request.
pub(crate) async fn serve() -> (SocketAddr, Arc<Mutex<Vec<Seen>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let app = axum::Router::new().fallback(move |request: Request| {
        let recorded = recorded.clone();
        async move {
            let (parts, body) = request.into_parts();
            let header = |name: &str| {
                let value = parts.headers.get(name).map(|v| v.to_str().unwrap());
                value.unwrap_or_default().to_owned()
            };
            let body = to_bytes(body, usize::MAX).await.unwrap();
            recorded.lock().unwrap().push(Seen {
                method: parts.method.to_string(),
                uri: parts.uri.to_string(),
                authorization: header("authorization"),
                accept: header("accept"),
                traceparent: header("traceparent"),
                body: String::from_utf8(body.to_vec()).unwrap(),
            });
            match parts.uri.path() {
                "/api/fail" => (AxumStatus::INTERNAL_SERVER_ERROR, "boom".to_owned()),
                "/api/large" => (
                    AxumStatus::OK,
                    "x".repeat(Wire::fixture().response_limit + 1),
                ),
                "/api/items" => (AxumStatus::CREATED, "created".to_owned()),
                _ => (AxumStatus::OK, r#"{"id":"found"}"#.to_owned()),
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, seen)
}

fn built(session: &Session<'_>, name: &str, args: Value) -> Result<reqwest::Request, Failure> {
    let (_, request) = session.prepare(name, args)?;
    Ok(request.build().unwrap())
}

#[tokio::test]
async fn builds_encoded_paths_declared_queries_and_json_bodies() {
    let target = target();
    let session = target.connect().await.unwrap();
    let request = built(
        &session,
        "getItem",
        json!({"path":{"id":"a b/c"},"query":{"limit":5}}),
    );
    let request = request.unwrap();
    assert_eq!(request.method(), Method::GET);
    assert_eq!(
        request.url().as_str(),
        format!("{BASE}/items/a%20b%2Fc?limit=5")
    );
    assert_eq!(request.headers()["authorization"], "Bearer test-token");
    assert!(request.headers()["authorization"].is_sensitive());
    assert_eq!(request.headers()["accept"], "application/json");
    assert!(request.body().is_none());

    let request = built(&session, "createItem", json!({"body":{"name":"n"}})).unwrap();
    assert_eq!(request.method(), Method::POST);
    assert_eq!(request.url().as_str(), format!("{BASE}/items"));
    assert_eq!(request.headers()["content-type"], "application/json");
    assert_eq!(
        request.body().unwrap().as_bytes().unwrap(),
        br#"{"name":"n"}"#
    );
}

#[tokio::test]
async fn rejects_arguments_the_operation_cannot_carry() {
    let target = target();
    let session = target.connect().await.unwrap();
    let oversized = json!({"body":{"pad":"x".repeat(Wire::fixture().request_limit)}});
    for (name, args) in [
        ("unknown", json!({})),
        ("getItem", json!([])),
        ("getItem", json!({})),
        ("getItem", json!({"path":{"id":".."}})),
        ("getItem", json!({"path":{"id":["a"]}})),
        ("getItem", json!({"path":{"id":"a"},"query":{"limit":[1]}})),
        ("getItem", json!({"path":{"id":"a"},"query":{"other":1}})),
        ("getItem", json!({"path":{"id":"a"},"body":{}})),
        ("getItem", json!({"path":{"id":"a"},"extra":1})),
        ("createItem", oversized),
    ] {
        assert_eq!(
            built(&session, name, args.clone()).err(),
            Some(Failure::Runner),
            "{args}"
        );
    }
}

#[tokio::test]
async fn read_only_operations_are_the_parallel_ones() {
    let target = target();
    let session = target.connect().await.unwrap();
    let names: Vec<_> = session
        .tools()
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(names.len(), 4);
    for name in ["getItem", "fail", "large"] {
        assert!(session.can_parallel(name), "{name}");
    }
    assert!(!session.can_parallel("createItem"));
    assert!(!session.can_parallel("unknown"));
}

#[tokio::test]
async fn calls_reach_the_api_with_session_trace_and_map_responses() {
    let (addr, seen) = serve().await;
    let target = Target::loopback(addr);
    let metrics = Metrics::new(TRAFFIC, &target.labels());
    let mut session = target.connect().await.unwrap();
    session.observe(&metrics);
    let get = |id: &str| {
        (
            "getItem".to_owned(),
            json!({"path":{"id":id},"query":{"limit":2}}),
        )
    };
    let results = session
        .call_group(vec![get("a b"), get("c")], &metrics)
        .await;
    assert_eq!(
        results.unwrap(),
        [
            json!({"status":200,"body":{"id":"found"}}),
            json!({"status":200,"body":{"id":"found"}})
        ]
    );
    let create = ("createItem".to_owned(), json!({"body":{"name":"n"}}));
    let created = session.call_group(vec![create.clone()], &metrics).await;
    assert_eq!(created.unwrap(), [json!({"status":201,"body":"created"})]);
    assert_eq!(
        session
            .call_group(vec![create.clone(), create], &metrics)
            .await,
        Err(Failure::Runner)
    );
    for name in ["fail", "large"] {
        let call = vec![(name.to_owned(), json!({}))];
        assert_eq!(
            session.call_group(call, &metrics).await,
            Err(Failure::Target)
        );
    }
    session.close().await.unwrap();

    let seen = seen.lock().unwrap();
    let mut uris: Vec<_> = seen
        .iter()
        .map(|s| (s.method.as_str(), s.uri.as_str()))
        .collect();
    uris[..2].sort_unstable();
    assert_eq!(
        uris,
        [
            ("GET", "/api/items/a%20b?limit=2"),
            ("GET", "/api/items/c?limit=2"),
            ("POST", "/api/items"),
            ("GET", "/api/fail"),
            ("GET", "/api/large"),
        ]
    );
    assert_eq!(seen[2].body, r#"{"name":"n"}"#);
    assert!(seen.iter().all(|s| s.authorization == "Bearer test-token"));
    assert!(seen.iter().all(|s| s.accept == "application/json"));
    let parents: Vec<Vec<&str>> = seen
        .iter()
        .map(|s| s.traceparent.split('-').collect())
        .collect();
    assert!(
        parents
            .iter()
            .all(|p| p.len() == 4 && p[1] == parents[0][1])
    );
    let mut spans: Vec<_> = parents.iter().map(|p| p[2]).collect();
    spans.sort_unstable();
    spans.dedup();
    assert_eq!(spans.len(), seen.len());

    let report = serde_json::to_value(metrics.snapshot(1, true)).unwrap();
    assert_eq!(report["hotspots"][0]["tool"], "getItem (GET /items/{id})");
    assert_eq!(report["hotspots"][0]["calls"], 2);
    let fail = report["hotspots"].as_array().unwrap().iter();
    let fail = fail
        .clone()
        .find(|row| row["tool"] == "fail (GET /fail)")
        .unwrap();
    assert_eq!(fail["target_errors"], 1);
    let sample = &report["traffic"]["session_samples"][0];
    assert_eq!(sample["trace_id"], parents[0][1]);
    assert_eq!(
        sample["dispatches"][0]["tools"],
        json!(["getItem", "getItem"])
    );
}
