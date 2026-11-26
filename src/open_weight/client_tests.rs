use super::*;
use crate::config::tests::{assert_each_required, env_of};
use serde_json::json;

pub(crate) const SETTINGS: [(&str, &str); 11] = [
    ("VLLM_CONNECT_TIMEOUT_SECONDS", "3"),
    ("VLLM_TIMEOUT_SECONDS", "20"),
    (
        "VLLM_URL",
        "https://inference.example.test/v1/chat/completions",
    ),
    ("VLLM_API_KEY", "fixture-token"),
    ("VLLM_MODEL", "fixture-model"),
    ("VLLM_MAX_TOKENS", "256"),
    ("VLLM_TEMPERATURE", "0.25"),
    ("VLLM_CHAT_TEMPLATE_KWARGS", r#"{"fixture_switch":false}"#),
    ("VLLM_REQUEST_LIMIT_BYTES", "131072"),
    ("VLLM_RESPONSE_LIMIT_BYTES", "65536"),
    ("VLLM_TOOL_ARGUMENTS_LIMIT_BYTES", "16384"),
];

impl Client {
    pub(crate) fn fixture(
        handler: impl Fn(Request) -> std::result::Result<Response, Failure> + Send + Sync + 'static,
    ) -> Self {
        Self {
            fixture: Some(Box::new(handler)),
            ..Self::from_env(&env_of(&SETTINGS)).unwrap()
        }
    }
}

fn response(status: StatusCode, body: impl Into<reqwest::Body>) -> Response {
    axum::http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap()
        .into()
}

fn raw(value: Value) -> Box<RawValue> {
    serde_json::value::to_raw_value(&value).unwrap()
}

fn completion(finish: &str, message: Value) -> Value {
    json!({
        "choices": [{"finish_reason": finish, "message": message}],
        "usage": {
            "prompt_tokens": 21, "completion_tokens": 12
        }
    })
}

#[tokio::test]
async fn builds_authenticated_request_and_parses_model_selected_tools() {
    let client = Client::fixture(|request| {
        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().path(), "/v1/chat/completions");
        assert!(request.headers()[header::AUTHORIZATION].is_sensitive());
        let body: Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["model"], "fixture-model");
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["max_tokens"], 256);
        assert_eq!(body["temperature"], 0.25);
        assert_eq!(
            body["chat_template_kwargs"],
            json!({"fixture_switch": false})
        );
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][0]["content"], "objective");
        assert_eq!(body["tools"][0]["function"]["name"], "lookup");
        Ok(response(StatusCode::OK, completion("tool_calls", json!({
            "role": "assistant", "content": null, "tool_calls": [
                {"id":"a", "type":"function", "function":{"name":"lookup","arguments":"{\"q\":1}"}},
                {"id":"b", "type":"function", "function":{"name":"lookup","arguments":"{\"q\":2}"}}
            ]
        })).to_string()))
    });
    let metrics = Metrics::default();
    let Turn::Calls(calls, message) = client
        .complete(
            &[raw(json!({"role":"system","content":"objective"}))],
            &raw(json!([{"type":"function","function":{"name":"lookup"}}])),
            &metrics,
        )
        .await
        .unwrap()
    else {
        panic!("expected model-selected calls")
    };
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "a");
    assert_eq!(calls[0].name, "lookup");
    assert_eq!(calls[1].args, json!({"q":2}));
    assert_eq!(message["role"], "assistant");
    assert_eq!(
        message["tool_calls"][1]["function"]["arguments"],
        "{\"q\":2}"
    );
    assert_eq!(message["tool_calls"][1]["type"], "function");
    assert_eq!(metrics.snapshot(1, true).tokens, 33);
}

#[tokio::test]
async fn handles_terminal_turn_and_inference_errors_without_retrying() {
    let metrics = Metrics::default();
    let client = Client::fixture(|_| {
        Ok(response(
            StatusCode::OK,
            completion("stop", json!({"role":"assistant","content":"Complete"})).to_string(),
        ))
    });
    assert!(matches!(
        client.complete(&[], &raw(json!([])), &metrics).await,
        Ok(Turn::Done)
    ));
    for (status, expected) in [
        (StatusCode::TOO_MANY_REQUESTS, Failure::InferenceThrottled),
        (StatusCode::BAD_GATEWAY, Failure::Inference),
    ] {
        let client = Client::fixture(move |_| Ok(response(status, "provider failure")));
        assert!(
            matches!(client.complete(&[], &raw(json!([])), &metrics).await, Err(error) if error == expected)
        );
    }
}

#[tokio::test]
async fn rejects_oversized_requests_and_responses() {
    let metrics = Metrics::default();
    let client = Client::fixture(|_| panic!("oversized request must not be sent"));
    assert!(matches!(
        client
            .complete(
                &[raw(json!("x".repeat(client.request_limit)))],
                &raw(json!([])),
                &metrics
            )
            .await,
        Err(Failure::Runner)
    ));
    let limit = Client::fixture(|_| unreachable!()).response_limit;
    let client = Client::fixture(move |_| Ok(response(StatusCode::OK, " ".repeat(limit + 1))));
    assert!(matches!(
        client.complete(&[], &raw(json!([])), &metrics).await,
        Err(Failure::Inference)
    ));
}

#[test]
fn rejects_bad_tool_calls_and_ambiguous_completions() {
    let tool = |id, arguments| json!({"id":id,"type":"function","function":{"name":"lookup","arguments":arguments}});
    for calls in [
        json!([]),
        json!([tool("a", "[]")]),
        json!([tool("a", "{broken")]),
        json!([tool("a", "{}"), tool("a", "{}")]),
    ] {
        let body = completion(
            "tool_calls",
            json!({"role":"assistant","content":null,"tool_calls":calls}),
        );
        assert!(matches!(
            parse_turn(
                serde_json::from_value(body).unwrap(),
                16,
                &Metrics::default()
            ),
            Err(Failure::Inference)
        ));
    }
    let long = completion(
        "tool_calls",
        json!({"role":"assistant","content":null,"tool_calls":[tool("a", r#"{"q":"0123456789"}"#)]}),
    );
    assert!(matches!(
        parse_turn(
            serde_json::from_value(long).unwrap(),
            16,
            &Metrics::default()
        ),
        Err(Failure::Inference)
    ));
    for finish in ["length", "content_filter", "stop"] {
        let body = completion(finish, json!({"role":"assistant","content":null}));
        assert!(matches!(
            parse_turn(
                serde_json::from_value(body).unwrap(),
                16,
                &Metrics::default()
            ),
            Err(Failure::Inference)
        ));
    }
}

#[test]
fn every_inference_setting_is_required() {
    assert_each_required(&SETTINGS, Client::from_env);
}

#[test]
fn configuration_requires_https_without_embedded_data_and_a_bearer() {
    assert!(endpoint("https://inference.example.test/v1/chat/completions").is_ok());
    assert!(bearer("\n").is_err());
    assert!(bearer("test-token").unwrap().is_sensitive());
    assert!(model(" ".into()).is_err());
    for invalid in [
        "http://example.test/v1/chat/completions",
        "https://secret@example.test/v1/chat/completions",
        "https://example.test/v1/chat/completions?q=secret",
        "https://example.test/v1/chat/completions#secret",
        "https://example.test/v1/models",
    ] {
        assert!(endpoint(invalid).is_err());
    }
}

#[tokio::test]
async fn sends_configured_decode_budget() {
    let client = Client {
        max_tokens: 64,
        temperature: 0.5,
        ..Client::fixture(|request| {
            let body: Value =
                serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
            assert_eq!(body["max_tokens"], 64);
            assert_eq!(body["temperature"], 0.5);
            Ok(response(
                StatusCode::OK,
                completion("stop", json!({"role":"assistant","content":"Complete"})).to_string(),
            ))
        })
    };
    assert!(matches!(
        client
            .complete(&[], &raw(json!([])), &Metrics::default())
            .await,
        Ok(Turn::Done)
    ));
}

#[test]
fn decode_settings_reject_invalid_values() {
    assert_eq!(temperature("0").unwrap(), 0.0);
    assert_eq!(temperature("2.0").unwrap(), 2.0);
    for invalid in ["-0.1", "2.01", "NaN", "inf", "warm"] {
        assert!(temperature(invalid).is_err());
    }
    assert_eq!(template_kwargs("{}").unwrap(), json!({}));
    for invalid in ["", "[]", "false", "{broken"] {
        assert!(template_kwargs(invalid).is_err());
    }
}

#[test]
fn sums_prompt_tokens_across_turns() {
    let metrics = Metrics::default();
    let done = json!({"role":"assistant","content":"Complete"});
    for _ in 0..3 {
        let completion = serde_json::from_value(completion("stop", done.clone())).unwrap();
        assert!(matches!(
            parse_turn(completion, 16, &metrics),
            Ok(Turn::Done)
        ));
    }
    assert_eq!(metrics.snapshot(1, true).input_tokens, 63);
}

#[test]
fn discards_reasoning_and_text_outside_tool_calls_and_counts_them() {
    let metrics = Metrics::default();
    let call = json!([{"id":"a","type":"function","function":{"name":"lookup","arguments":"{}"}}]);
    let turn = |message: Value, details: Option<Value>| {
        let mut body = completion("tool_calls", message);
        if let Some(details) = details {
            body["usage"]["completion_tokens_details"] = details;
        }
        parse_turn(serde_json::from_value(body).unwrap(), 16, &metrics)
    };
    let leaky = json!({"role":"assistant","content":"Let me look.","reasoning_content":"hmm",
        "tool_calls":call});
    let Ok(Turn::Calls(_, message)) = turn(leaky, Some(json!({"reasoning_tokens": 7}))) else {
        panic!("expected calls")
    };
    assert_eq!(
        message,
        json!({"role":"assistant","content":null,"tool_calls":call})
    );
    let reasoning = json!({"role":"assistant","content":null,"reasoning":"hmm","tool_calls":call});
    assert!(matches!(
        turn(reasoning, Some(json!(null))),
        Ok(Turn::Calls(..))
    ));
    let clean = json!({"role":"assistant","content":"","tool_calls":call});
    assert!(matches!(turn(clean, None), Ok(Turn::Calls(..))));
    let report = serde_json::to_value(metrics.snapshot(1, true)).unwrap();
    assert_eq!(
        report["reasoning_discarded"],
        json!({"messages": 2, "tokens": 7})
    );
}
