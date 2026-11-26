use super::{Metrics, PassCriteria, TrafficShape};
use std::time::Duration;

pub(crate) const DRAIN: Duration = Duration::from_secs(5);

pub(crate) const CRITERIA: PassCriteria = PassCriteria {
    runner_p95_ms: 50,
    runner_p99_ms: 100,
    error_budget_percent: 1,
};

pub(crate) const TRAFFIC: TrafficShape = TrafficShape {
    window_ms: 100,
    windows: 3051,
    session_samples: 16,
};

impl Default for Metrics {
    fn default() -> Self {
        Self::new(TRAFFIC, &[])
    }
}
