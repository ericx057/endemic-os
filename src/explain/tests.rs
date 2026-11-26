use super::*;
use crate::runner::{Metrics, fixture::TRAFFIC};
use serde_json::json;
use std::time::Duration;

type Calls<'a> = &'a [(&'a str, u64, u64, u64)];
type Traces<'a> = &'a [(&'a str, &'a [&'a str])];

fn stage(
    agents: usize,
    failed: Option<Criterion>,
    calls: Calls,
    traces: Traces,
) -> Arc<StageReport> {
    let labels = ["lookup", "refund", "search"].map(String::from);
    let metrics = Metrics::new(TRAFFIC, &labels);
    for &(tool, ok, errors, ms) in calls {
        for index in 0..ok + errors {
            metrics.tool_call(tool, Duration::from_millis(ms), index >= errors);
        }
    }
    for (trace_id, tools) in traces {
        metrics
            .trace(trace_id)
            .unwrap()
            .record(tools.iter().copied());
    }
    Arc::new(StageReport {
        result: metrics.snapshot(agents, true),
        hold_seconds: 1,
        passed: failed.is_none(),
        failed_criterion: failed,
        interrupted: 0,
        faults: Vec::new(),
    })
}

fn steady(agents: usize) -> Arc<StageReport> {
    stage(
        agents,
        None,
        &[
            ("search", 10, 0, 5),
            ("refund", 10, 0, 5),
            ("lookup", 10, 0, 5),
        ],
        &[],
    )
}

#[test]
fn names_the_failed_criterion_the_tools_that_rose_most_and_where_the_rise_began() {
    let stages = [
        steady(1),
        steady(2),
        stage(
            4,
            None,
            &[
                ("search", 9, 1, 5),
                ("refund", 10, 0, 40),
                ("lookup", 10, 0, 5),
            ],
            &[],
        ),
        stage(
            8,
            Some(Criterion::ErrorBudget),
            &[
                ("search", 5, 5, 5),
                ("refund", 10, 0, 200),
                ("lookup", 10, 0, 5),
            ],
            &[
                ("trace-search", &["search"]),
                ("trace-lookup", &["lookup"]),
                ("trace-refund", &["lookup", "refund"]),
            ],
        ),
    ];
    let explanation = serde_json::to_value(explain(&stages)).unwrap();
    assert_eq!(
        explanation,
        json!({
            "agents": 8,
            "criterion": "error_budget",
            "baseline_agents": 4,
            "error_rate": {"tool": "search", "from": 0.1, "to": 0.5, "began_at_agents": 4},
            "p95_ms": {"tool": "refund", "from": 40.0, "to": 200.0, "began_at_agents": 4},
            "sample_trace_ids": ["trace-search", "trace-refund"],
        })
    );
}

#[test]
fn a_failing_first_stage_ranks_tools_by_their_own_values() {
    let stages = [stage(
        2,
        Some(Criterion::RunnerP95),
        &[("search", 8, 2, 5), ("refund", 10, 0, 40)],
        &[("trace-refund", &["refund"])],
    )];
    let explanation = explain(&stages).unwrap();
    assert_eq!(explanation.baseline_agents, None);
    assert_eq!(
        explanation.error_rate,
        Some(Rise {
            tool: "search".into(),
            from: None,
            to: 0.2,
            began_at_agents: 2,
        })
    );
    assert_eq!(
        explanation.p95_ms.map(|rise| (rise.tool, rise.to)),
        Some(("refund".into(), 40.0))
    );
    assert_eq!(explanation.sample_trace_ids, ["trace-refund"]);
}

#[test]
fn a_failure_without_any_tool_regression_still_names_its_criterion() {
    let stages = [
        steady(1),
        stage(
            2,
            Some(Criterion::AllAgentsActive),
            &[("search", 10, 0, 5)],
            &[("trace-search", &["search"])],
        ),
    ];
    let explanation = explain(&stages).unwrap();
    assert_eq!(explanation.criterion, Criterion::AllAgentsActive);
    assert_eq!(explanation.baseline_agents, Some(1));
    assert_eq!((explanation.error_rate, explanation.p95_ms), (None, None));
    assert!(explanation.sample_trace_ids.is_empty());
}

#[test]
fn a_rise_that_dipped_begins_at_the_failing_stage_and_passing_runs_have_none() {
    let stages = [
        steady(1),
        stage(2, None, &[("refund", 10, 0, 90)], &[]),
        stage(4, None, &[("refund", 10, 0, 20)], &[]),
        stage(8, Some(Criterion::RunnerP99), &[("refund", 10, 0, 60)], &[]),
    ];
    let rise = explain(&stages).unwrap().p95_ms.unwrap();
    assert_eq!(
        (rise.from, rise.to, rise.began_at_agents),
        (Some(20.0), 60.0, 8)
    );
    assert_eq!(explain(&stages[..3]), None);
}
