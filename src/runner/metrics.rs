use super::{
    faults::{Effect, Injected, Injector, StageFaults},
    hotspots::{Hotspot, Hotspots},
    traffic::{ToolTrace, Traffic, TrafficReport, TrafficShape},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Duration,
};

const EXACT_MS: usize = 256;
const BUCKETS: usize = EXACT_MS + 24;

pub(super) struct Histogram([AtomicU64; BUCKETS]);

impl Default for Histogram {
    fn default() -> Self {
        Self(std::array::from_fn(|_| AtomicU64::new(0)))
    }
}

impl Histogram {
    pub(super) fn record(&self, latency: Duration) {
        let ms = latency.as_nanos().div_ceil(1_000_000);
        let bucket = if ms < EXACT_MS as u128 {
            ms as usize
        } else {
            EXACT_MS + (ms.ilog2() as usize - 8).min(BUCKETS - EXACT_MS - 1)
        };
        self.0[bucket].fetch_add(1, Relaxed);
    }

    pub(super) fn summary(&self) -> Latencies {
        let buckets = self.0.each_ref().map(|bucket| bucket.load(Relaxed));
        let samples: u64 = buckets.iter().sum();
        let percentile = |percent: u64| {
            if samples == 0 {
                return None;
            }
            let rank = (samples * percent).div_ceil(100);
            let mut cumulative = 0;
            for (index, count) in buckets.iter().enumerate() {
                cumulative += count;
                if cumulative >= rank {
                    return Some(if index < EXACT_MS {
                        index as u64
                    } else {
                        (1 << (index - EXACT_MS + 9)) - 1
                    });
                }
            }
            None
        };
        Latencies {
            samples,
            p95_ms: percentile(95),
            p99_ms: percentile(99),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Target,
    Inference,
    InferenceThrottled,
    Runner,
    Injected,
}

pub struct Metrics {
    overhead: Histogram,
    target: Histogram,
    inference: Histogram,
    input_tokens: AtomicU64,
    output_tokens: AtomicU64,
    discarded_messages: AtomicU64,
    discarded_tokens: AtomicU64,
    traffic: Traffic,
    hotspots: Hotspots,
    injector: Injector,
    started: AtomicU64,
    success: AtomicU64,
    target_errors: AtomicU64,
    inference_errors: AtomicU64,
    inference_throttles: AtomicU64,
    runner_errors: AtomicU64,
    injected_failures: AtomicU64,
    active: AtomicU64,
    peak: AtomicU64,
}

pub(super) struct ActiveAgent<'a>(&'a AtomicU64);

impl Drop for ActiveAgent<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Relaxed);
    }
}

impl Metrics {
    pub fn new(traffic: TrafficShape, labels: &[String]) -> Self {
        Self {
            overhead: Histogram::default(),
            target: Histogram::default(),
            inference: Histogram::default(),
            input_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            discarded_messages: AtomicU64::new(0),
            discarded_tokens: AtomicU64::new(0),
            traffic: Traffic::new(traffic),
            hotspots: Hotspots::new(labels),
            injector: Injector::new(StageFaults::default()),
            started: AtomicU64::new(0),
            success: AtomicU64::new(0),
            target_errors: AtomicU64::new(0),
            inference_errors: AtomicU64::new(0),
            inference_throttles: AtomicU64::new(0),
            runner_errors: AtomicU64::new(0),
            injected_failures: AtomicU64::new(0),
            active: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        }
    }

    pub fn with_faults(self, faults: StageFaults) -> Self {
        Self {
            injector: Injector::new(faults),
            ..self
        }
    }

    pub(crate) fn inject(&self, tool: &str, call: u64) -> Option<Effect> {
        self.injector.draw(tool, call)
    }

    pub fn overhead(&self, elapsed: Duration) {
        self.overhead.record(elapsed);
    }
    pub fn target(&self, elapsed: Duration) {
        self.target.record(elapsed);
    }
    pub fn inference(&self, elapsed: Duration) {
        self.inference.record(elapsed);
    }
    pub fn inference_tokens(&self, input: u64, output: u64) {
        self.input_tokens.fetch_add(input, Relaxed);
        self.output_tokens.fetch_add(output, Relaxed);
    }
    /// `messages` completions had reasoning or text dropped; `tokens` is what usage reported.
    pub fn reasoning_discarded(&self, messages: u64, tokens: u64) {
        self.discarded_messages.fetch_add(messages, Relaxed);
        self.discarded_tokens.fetch_add(tokens, Relaxed);
    }
    /// One tool call: feeds target latency and the per-tool hotspots.
    pub(crate) fn tool_call(&self, tool: &str, elapsed: Duration, ok: bool) {
        self.target.record(elapsed);
        self.hotspots.record(tool, elapsed, ok);
    }
    pub(crate) fn trace(&self, trace_id: &str) -> Option<ToolTrace<'_>> {
        self.traffic.trace(trace_id)
    }
    pub(crate) fn tool_batch(&self, count: usize) {
        self.traffic.batch(count);
    }
    pub(super) fn session_started(&self) {
        self.started.fetch_add(1, Relaxed);
    }

    pub(super) fn agent_started(&self) -> ActiveAgent<'_> {
        self.peak
            .fetch_max(self.active.fetch_add(1, Relaxed) + 1, Relaxed);
        ActiveAgent(&self.active)
    }

    pub(crate) fn finished(&self, result: Result<(), Failure>) {
        match result {
            Ok(()) => &self.success,
            Err(Failure::Target) => &self.target_errors,
            Err(Failure::Inference) => &self.inference_errors,
            Err(Failure::InferenceThrottled) => &self.inference_throttles,
            Err(Failure::Runner) => &self.runner_errors,
            Err(Failure::Injected) => &self.injected_failures,
        }
        .fetch_add(1, Relaxed);
    }

    pub(crate) fn snapshot(&self, agents: usize, held: bool) -> StageResult {
        let input_tokens = self.input_tokens.load(Relaxed);
        let output_tokens = self.output_tokens.load(Relaxed);
        StageResult {
            agents,
            held,
            peak_active: self.peak.load(Relaxed),
            started: self.started.load(Relaxed),
            completed: self.success.load(Relaxed),
            runner_errors: self.runner_errors.load(Relaxed),
            injected_failures: self.injected_failures.load(Relaxed),
            target_errors: self.target_errors.load(Relaxed),
            inference_errors: self.inference_errors.load(Relaxed),
            inference_throttles: self.inference_throttles.load(Relaxed),
            tokens: input_tokens + output_tokens,
            input_tokens,
            output_tokens,
            reasoning_discarded: ReasoningDiscarded {
                messages: self.discarded_messages.load(Relaxed),
                tokens: self.discarded_tokens.load(Relaxed),
            },
            traffic: self.traffic.report(),
            hotspots: self.hotspots.report(),
            injected: self.injector.counts(),
            runner: self.overhead.summary(),
            target: self.target.summary(),
            inference: self.inference.summary(),
        }
    }
}

/// Completions whose reasoning or text outside tool calls was dropped before history.
#[derive(Clone, Serialize)]
pub struct ReasoningDiscarded {
    pub messages: u64,
    /// From `usage.completion_tokens_details.reasoning_tokens`; 0 when vLLM omits it.
    pub tokens: u64,
}

#[derive(Clone, Serialize)]
pub struct Latencies {
    pub(super) samples: u64,
    pub(super) p95_ms: Option<u64>,
    pub(super) p99_ms: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct StageResult {
    pub agents: usize,
    pub held: bool,
    pub peak_active: u64,
    pub started: u64,
    pub completed: u64,
    pub runner_errors: u64,
    pub injected_failures: u64,
    pub target_errors: u64,
    pub inference_errors: u64,
    pub inference_throttles: u64,
    pub tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_discarded: ReasoningDiscarded,
    pub traffic: TrafficReport,
    /// Tools agents called, most visited first.
    pub hotspots: Vec<Hotspot>,
    pub injected: BTreeMap<String, Injected>,
    pub runner: Latencies,
    pub target: Latencies,
    pub inference: Latencies,
}

/// What a stage must show to pass.
#[derive(Clone, Copy)]
pub struct PassCriteria {
    /// Runner overhead percentiles: the runner must not be the bottleneck.
    pub runner_p95_ms: u64,
    pub runner_p99_ms: u64,
    /// Errors plus interrupted sessions must stay below this percent of all finished sessions.
    pub error_budget_percent: u64,
}

impl StageResult {
    pub fn interrupted(&self) -> u64 {
        self.started
            .saturating_sub(self.completed + self.errors() + self.injected_failures)
    }

    fn errors(&self) -> u64 {
        self.runner_errors + self.target_errors + self.inference_errors + self.inference_throttles
    }

    #[cfg(test)]
    pub fn passes(&self, criteria: &PassCriteria) -> bool {
        self.failed_criterion(criteria).is_none()
    }

    pub fn failed_criterion(&self, criteria: &PassCriteria) -> Option<Criterion> {
        let budget = criteria.error_budget_percent;
        let within = |value: Option<u64>, limit| value.is_some_and(|value| value <= limit);
        [
            (Criterion::Held, self.held),
            (
                Criterion::AllAgentsActive,
                self.peak_active == self.agents as u64,
            ),
            (Criterion::SessionsCompleted, self.completed > 0),
            (
                Criterion::RunnerP95,
                within(self.runner.p95_ms, criteria.runner_p95_ms),
            ),
            (
                Criterion::RunnerP99,
                within(self.runner.p99_ms, criteria.runner_p99_ms),
            ),
            (
                Criterion::ErrorBudget,
                (self.errors() + self.interrupted()) * (100 - budget) < self.completed * budget,
            ),
        ]
        .into_iter()
        .find_map(|(criterion, met)| (!met).then_some(criterion))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Criterion {
    Held,
    AllAgentsActive,
    SessionsCompleted,
    RunnerP95,
    RunnerP99,
    ErrorBudget,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::fixture::CRITERIA;

    #[test]
    fn the_first_unmet_criterion_is_named() {
        let metrics = Metrics::default();
        assert_eq!(
            metrics.snapshot(1, false).failed_criterion(&CRITERIA),
            Some(Criterion::Held)
        );
        assert_eq!(
            metrics.snapshot(1, true).failed_criterion(&CRITERIA),
            Some(Criterion::AllAgentsActive)
        );
        let _active = metrics.agent_started();
        assert_eq!(
            metrics.snapshot(1, true).failed_criterion(&CRITERIA),
            Some(Criterion::SessionsCompleted)
        );
        metrics.finished(Ok(()));
        assert_eq!(
            metrics.snapshot(1, true).failed_criterion(&CRITERIA),
            Some(Criterion::RunnerP95)
        );
        for index in 0..100 {
            let ms = if index < 98 { 5 } else { 150 };
            metrics.overhead(Duration::from_millis(ms));
        }
        assert_eq!(
            metrics.snapshot(1, true).failed_criterion(&CRITERIA),
            Some(Criterion::RunnerP99)
        );
        let fast = Metrics::default();
        let _active = fast.agent_started();
        fast.overhead(Duration::from_millis(5));
        fast.finished(Ok(()));
        assert_eq!(fast.snapshot(1, true).failed_criterion(&CRITERIA), None);
        fast.finished(Err(Failure::Target));
        assert_eq!(
            fast.snapshot(1, true).failed_criterion(&CRITERIA),
            Some(Criterion::ErrorBudget)
        );
    }

    #[test]
    fn capacity_checks_both_latency_percentiles_and_strict_error_budget() {
        let metrics = Metrics::default();
        let _active = metrics.agent_started();
        for _ in 0..99 {
            metrics.overhead(Duration::from_millis(50));
            metrics.finished(Ok(()));
        }
        assert!(metrics.snapshot(1, true).passes(&CRITERIA));
        metrics.finished(Err(Failure::Target));
        assert!(!metrics.snapshot(1, true).passes(&CRITERIA));
        metrics.finished(Ok(()));
        assert!(metrics.snapshot(1, true).passes(&CRITERIA));

        for (fast_count, slow_latency) in [(94, 51), (98, 101)] {
            let metrics = Metrics::default();
            let _active = metrics.agent_started();
            for index in 0..100 {
                let ms = if index < fast_count { 50 } else { slow_latency };
                metrics.overhead(Duration::from_millis(ms));
                metrics.finished(Ok(()));
            }
            assert!(!metrics.snapshot(1, true).passes(&CRITERIA));
        }
    }
}
