//! Which parts of the target agents visit most: per-tool call counts, errors, and latency.

use super::metrics::{Histogram, Latencies};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Duration,
};

#[derive(Default)]
struct Spot {
    errors: AtomicU64,
    latency: Histogram,
}

pub(super) struct Hotspots(BTreeMap<String, Spot>);

impl Hotspots {
    pub(super) fn new(labels: &[String]) -> Self {
        Self(
            labels
                .iter()
                .map(|label| (label.clone(), Spot::default()))
                .collect(),
        )
    }

    pub(super) fn record(&self, tool: &str, elapsed: Duration, ok: bool) {
        let Some(spot) = self.0.get(tool) else {
            return;
        };
        spot.latency.record(elapsed);
        if !ok {
            spot.errors.fetch_add(1, Relaxed);
        }
    }

    pub(super) fn report(&self) -> Vec<Hotspot> {
        let rows: Vec<(String, Latencies, u64)> = self
            .0
            .iter()
            .map(|(tool, spot)| {
                (
                    tool.clone(),
                    spot.latency.summary(),
                    spot.errors.load(Relaxed),
                )
            })
            .filter(|(_, latency, _)| latency.samples > 0)
            .collect();
        let total: u64 = rows.iter().map(|(_, latency, _)| latency.samples).sum();
        let mut hotspots: Vec<Hotspot> = rows
            .into_iter()
            .map(|(tool, latency, target_errors)| Hotspot {
                tool,
                calls: latency.samples,
                share: latency.samples as f64 / total.max(1) as f64,
                target_errors,
                p95_ms: latency.p95_ms,
                p99_ms: latency.p99_ms,
            })
            .collect();
        hotspots.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.tool.cmp(&b.tool)));
        hotspots
    }
}

#[derive(Clone, Serialize)]
pub struct Hotspot {
    pub(crate) tool: String,
    pub(crate) calls: u64,
    /// Fraction (0..=1) of all tool calls in the stage.
    share: f64,
    pub(crate) target_errors: u64,
    pub(crate) p95_ms: Option<u64>,
    p99_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::super::{Metrics, fixture::TRAFFIC};
    use std::time::Duration;

    #[test]
    fn ranks_tools_by_calls_with_share_errors_and_latency() {
        let labels = ["search", "refund", "unused"].map(String::from);
        let metrics = Metrics::new(TRAFFIC, &labels);
        for _ in 0..3 {
            metrics.tool_call("search", Duration::from_millis(5), true);
        }
        metrics.tool_call("refund", Duration::from_millis(40), false);
        metrics.tool_call("unplanned", Duration::from_millis(7), true);
        let report = serde_json::to_value(metrics.snapshot(1, true)).unwrap();
        assert_eq!(
            report["hotspots"],
            serde_json::json!([
                {"tool": "search", "calls": 3, "share": 0.75, "target_errors": 0, "p95_ms": 5, "p99_ms": 5},
                {"tool": "refund", "calls": 1, "share": 0.25, "target_errors": 1, "p95_ms": 40, "p99_ms": 40},
            ])
        );
        assert_eq!(
            report["target"]["samples"], 5,
            "tool calls still feed target latency"
        );
    }
}
