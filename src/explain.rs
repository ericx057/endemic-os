use super::StageReport;
use crate::runner::Criterion;
use serde::Serialize;
use std::sync::Arc;

type Metric = fn(&str, &StageReport) -> Option<f64>;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct Explanation {
    agents: usize,
    criterion: Criterion,
    baseline_agents: Option<usize>,
    error_rate: Option<Rise>,
    p95_ms: Option<Rise>,
    sample_trace_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct Rise {
    tool: String,
    from: Option<f64>,
    to: f64,
    began_at_agents: usize,
}

pub(super) fn explain(stages: &[Arc<StageReport>]) -> Option<Explanation> {
    let (failed, criterion) = stages
        .iter()
        .enumerate()
        .find_map(|(index, stage)| stage.failed_criterion.map(|criterion| (index, criterion)))?;
    let history = &stages[..=failed];
    let baseline = history.iter().rposition(|stage| stage.passed);
    let error_rate = rise(history, baseline, tool_error_rate);
    let p95_ms = rise(history, baseline, tool_p95_ms);
    let suspects: Vec<&str> = [&error_rate, &p95_ms]
        .into_iter()
        .flatten()
        .map(|rise| rise.tool.as_str())
        .collect();
    Some(Explanation {
        agents: stages[failed].result.agents,
        criterion,
        baseline_agents: baseline.map(|index| stages[index].result.agents),
        sample_trace_ids: traces_calling(&stages[failed], &suspects),
        error_rate,
        p95_ms,
    })
}

fn tool_error_rate(tool: &str, stage: &StageReport) -> Option<f64> {
    let spot = stage
        .result
        .hotspots
        .iter()
        .find(|spot| spot.tool == tool)?;
    (spot.calls > 0).then(|| spot.target_errors as f64 / spot.calls as f64)
}

fn tool_p95_ms(tool: &str, stage: &StageReport) -> Option<f64> {
    let spot = stage
        .result
        .hotspots
        .iter()
        .find(|spot| spot.tool == tool)?;
    spot.p95_ms.map(|ms| ms as f64)
}

fn rise(history: &[Arc<StageReport>], baseline: Option<usize>, metric: Metric) -> Option<Rise> {
    let failed = history.last()?;
    failed
        .result
        .hotspots
        .iter()
        .filter_map(|spot| {
            let to = metric(&spot.tool, failed)?;
            let from = match baseline {
                Some(index) => Some(metric(&spot.tool, &history[index])?),
                None => None,
            };
            let gain = from.map_or(to, |from| to - from);
            (gain > 0.0).then_some((spot.tool.as_str(), from, to, gain))
        })
        .max_by(|a, b| a.3.total_cmp(&b.3).then_with(|| b.0.cmp(a.0)))
        .map(|(tool, from, to, _)| Rise {
            tool: tool.to_owned(),
            from,
            to,
            began_at_agents: began(history, tool, metric),
        })
}

fn began(history: &[Arc<StageReport>], tool: &str, metric: Metric) -> usize {
    let values: Vec<Option<f64>> = history.iter().map(|stage| metric(tool, stage)).collect();
    let rising = values
        .windows(2)
        .rev()
        .take_while(|pair| matches!(pair, [Some(before), Some(after)] if before < after))
        .count();
    history[history.len() - rising.max(1)].result.agents
}

fn traces_calling(stage: &StageReport, tools: &[&str]) -> Vec<String> {
    stage
        .result
        .traffic
        .session_samples
        .iter()
        .filter(|sample| {
            sample
                .dispatches
                .iter()
                .flat_map(|dispatch| &dispatch.tools)
                .any(|tool| tools.contains(&tool.as_str()))
        })
        .map(|sample| sample.trace_id.clone())
        .collect()
}

#[cfg(test)]
#[path = "explain/tests.rs"]
mod tests;
