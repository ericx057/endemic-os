use super::*;
use crate::open_weight::client::Client;
use axum::http::Response as HttpResponse;
use serde_json::json;

fn tool(name: &str, description: Option<&str>) -> Tool {
    Tool {
        name: name.into(),
        description: description.map(str::to_owned),
        schema: json!({"type":"object"}),
        read_only: false,
    }
}

fn reply(finish: &str, content: &str) -> reqwest::Response {
    let body = json!({
        "choices":[{"finish_reason":finish,"message":{"role":"assistant","content":content}}],
        "usage":{"prompt_tokens":10,"completion_tokens":5}
    });
    HttpResponse::builder()
        .header("content-type", "application/json")
        .body(body.to_string())
        .unwrap()
        .into()
}

fn engine(content: &'static str) -> Engine {
    Engine {
        client: Client::fixture(move |_| Ok(reply("stop", content))),
        ..Engine::fixture()
    }
}

#[tokio::test]
async fn asks_once_for_a_json_array_given_tools_and_use_case() {
    let engine = Engine {
        client: Client::fixture(|request| {
            let body: Value =
                serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
            assert!(body.get("tools").is_none() && body.get("tool_choice").is_none());
            assert_eq!(body["response_format"]["type"], "json_schema");
            assert_eq!(
                body["response_format"]["json_schema"]["schema"],
                json!({"type":"array","items":{"type":"string"}})
            );
            assert!(
                body["messages"][0]["content"]
                    .as_str()
                    .unwrap()
                    .contains("exactly 2")
            );
            let user: Value =
                serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert_eq!(
                user,
                json!({
                    "tools":[{"name":"search","description":"Find orders"},{"name":"refund","description":null}],
                    "use_case":"Shoppers refund late orders."
                })
            );
            Ok(reply("stop", r#" ["Find order 1.", "Refund order 2."] "#))
        }),
        ..Engine::fixture()
    };
    let tools = [tool("search", Some("Find orders")), tool("refund", None)];
    let tasks = engine
        .tasks(&tools, Some("Shoppers refund late orders."), 2)
        .await
        .unwrap();
    assert_eq!(tasks, ["Find order 1.", "Refund order 2."]);
}

#[tokio::test]
async fn without_a_use_case_the_tools_alone_steer_the_tasks() {
    let engine = Engine {
        client: Client::fixture(|request| {
            let body: Value =
                serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
            let user: Value =
                serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert_eq!(user["use_case"], Value::Null);
            Ok(reply("stop", r#"["Search."]"#))
        }),
        ..Engine::fixture()
    };
    assert_eq!(
        engine
            .tasks(&[tool("search", None)], None, 1)
            .await
            .unwrap(),
        ["Search."]
    );
}

#[tokio::test]
async fn anything_but_exactly_n_distinct_bounded_tasks_fails_the_run() {
    let long = Box::leak(format!(r#"["{}","b"]"#, "x".repeat(TASK_LIMIT + 1)).into_boxed_str());
    for (content, expected) in [
        (r#"["a"]"#, "returned 1 tasks, expected 2"),
        (r#"["a","b","c"]"#, "returned 3 tasks, expected 2"),
        (r#"["a","  "]"#, "task 2 is blank"),
        (r#"["a","a"]"#, "task 2 repeats an earlier task"),
        (&*long, "task 1 is longer than 4096 bytes"),
        (r#"{"tasks":["a","b"]}"#, "not a JSON array of strings"),
        ("```json\n[\"a\",\"b\"]\n```", "not a JSON array of strings"),
        (r#"["a",2]"#, "not a JSON array of strings"),
    ] {
        let error = engine(content)
            .tasks(&[tool("search", None)], None, 2)
            .await
            .unwrap_err();
        assert!(error.starts_with("task generation failed: "), "{error}");
        assert!(error.contains(expected), "{content}: {error}");
    }
}

#[tokio::test]
async fn an_unusable_completion_fails_with_the_inference_reason() {
    let engine = Engine {
        client: Client::fixture(|_| Ok(reply("length", r#"["a""#))),
        ..Engine::fixture()
    };
    let error = engine.tasks(&[tool("s", None)], None, 1).await.unwrap_err();
    assert!(error.contains("no usable completion"), "{error}");

    let engine = Engine {
        client: Client::fixture(|_| Err(Failure::InferenceThrottled)),
        ..Engine::fixture()
    };
    let error = engine.tasks(&[tool("s", None)], None, 1).await.unwrap_err();
    assert!(error.contains("throttled"), "{error}");
}
