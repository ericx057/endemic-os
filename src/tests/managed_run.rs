//! End to end through the API: POST /runs plans against a loopback MCP server and a scripted
//! vLLM, then ramps agents that call the target. No network beyond loopback.

use super::*;
use crate::{
    open_weight::Engine,
    plan::{
        Planner,
        tests::{TASKS, inference, mcp_server, mcp_tools},
    },
    request::tests::FIXTURE_TOKEN,
};
use std::sync::Mutex;

#[tokio::test]
async fn managed_run_plans_ramps_and_reports_without_the_token() {
    let (addr, seen) = mcp_server(mcp_tools()).await;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let planner = Arc::new(Planner::loopback(Engine::scripted(inference(
        requests.clone(),
    ))));
    let app = App::new(Settings::ramp(1, 2), move |request| {
        let planner = planner.clone();
        async move { planner.prepare(request).await }
    });
    let request = RunRequest::loopback(&format!("http://{addr}/mcp"), None, None);
    let started = start_run(app.clone(), request).await.unwrap();
    let report = finished(&app, started.id, Duration::from_secs(10)).await;
    let json = serde_json::to_value(&report).unwrap();

    assert_eq!(json["status"], "reached_ceiling", "{json}");
    assert_eq!(json["error"], serde_json::Value::Null);
    assert_eq!(
        json["plan"],
        serde_json::json!({"target_kind":"mcp","tools":["search","refund"],
            "parallel_tools":["search"],"tasks":TASKS,"hold_agents":null,"pacing":{"start_spread_ms":0,"think_min_ms":0,"think_max_ms":0,"sync_sessions":false},"agents_per_stage":[1,2]})
    );
    assert_eq!(json["highest_passing_agents"], 2);
    let stages = json["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 2);
    assert!(stages.iter().all(|stage| stage["passed"] == true));
    assert!(stages[1]["traffic"]["tool_calls"].as_u64().unwrap() > 0);
    assert_eq!(stages[1]["hotspots"][0]["tool"], "search");
    assert!(!json.to_string().contains(FIXTURE_TOKEN));

    // Both tasks were pursued in the two-agent stage, and the target saw real tool calls.
    let objectives: Vec<String> = requests
        .lock()
        .unwrap()
        .iter()
        .filter(|body| body.get("tools").is_some())
        .map(|body| body["messages"][1]["content"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        TASKS
            .iter()
            .all(|task| objectives.iter().any(|o| o == task))
    );
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|(method, _)| method == "tools/call"));
}
