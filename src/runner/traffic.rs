use serde::Serialize;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::Instant,
};

#[derive(Clone, Copy)]
pub struct TrafficShape {
    pub window_ms: u64,
    pub windows: usize,
    pub session_samples: u64,
}

pub(super) struct Traffic {
    started: tokio::time::Instant,
    window_ms: u64,
    session_samples: u64,
    windows: Box<[AtomicU64]>,
    tool_calls: AtomicU64,
    trace_slots: AtomicU64,
    traces: Mutex<Vec<SessionSample>>,
}

impl Traffic {
    pub(super) fn new(shape: TrafficShape) -> Self {
        Self {
            started: tokio::time::Instant::now(),
            window_ms: shape.window_ms,
            session_samples: shape.session_samples,
            windows: (0..shape.windows).map(|_| AtomicU64::new(0)).collect(),
            tool_calls: AtomicU64::new(0),
            trace_slots: AtomicU64::new(0),
            traces: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn batch(&self, count: usize) {
        let window = self.started.elapsed().as_millis() / u128::from(self.window_ms);
        if let Some(bucket) = self.windows.get(window as usize) {
            bucket.fetch_add(count as u64, Relaxed);
        }
        self.tool_calls.fetch_add(count as u64, Relaxed);
    }

    pub(super) fn trace(&self, trace_id: &str) -> Option<ToolTrace<'_>> {
        let limit = self.session_samples;
        if self.trace_slots.load(Relaxed) >= limit {
            return None;
        }
        self.trace_slots
            .try_update(Relaxed, Relaxed, |used| (used < limit).then_some(used + 1))
            .ok()?;
        Some(ToolTrace {
            traffic: self,
            started: Instant::now(),
            sample: SessionSample {
                trace_id: trace_id.to_owned(),
                dispatches: Vec::new(),
            },
        })
    }

    pub(super) fn report(&self) -> TrafficReport {
        let mut calls_per_window: Vec<u64> = self
            .windows
            .iter()
            .map(|window| window.load(Relaxed))
            .collect();
        let busy = calls_per_window.iter().rposition(|count| *count != 0);
        calls_per_window.truncate(busy.map_or(0, |last| last + 1));
        let mut occupied: Vec<u64> = calls_per_window
            .iter()
            .copied()
            .filter(|count| *count != 0)
            .collect();
        occupied.sort_unstable();
        let rank = (occupied.len() * 95).div_ceil(100);
        let tool_calls = self.tool_calls.load(Relaxed);
        TrafficReport {
            tool_calls,
            window_ms: self.window_ms,
            unbucketed_tool_calls: tool_calls.saturating_sub(occupied.iter().sum()),
            peak_calls_per_window: occupied.last().copied().unwrap_or(0),
            p95_calls_per_occupied_window: rank.checked_sub(1).map(|index| occupied[index]),
            session_samples: self.traces.lock().expect("tool traces lock").clone(),
            calls_per_window,
        }
    }
}

pub(crate) struct ToolTrace<'a> {
    traffic: &'a Traffic,
    started: Instant,
    sample: SessionSample,
}

impl ToolTrace<'_> {
    pub(crate) fn record<'a>(&mut self, names: impl Iterator<Item = &'a str>) {
        let at_us = self.started.elapsed().as_micros();
        self.sample.dispatches.push(Dispatch {
            at_us,
            tools: names.map(str::to_owned).collect(),
        });
    }
}

impl Drop for ToolTrace<'_> {
    fn drop(&mut self) {
        if !self.sample.dispatches.is_empty() {
            self.traffic
                .traces
                .lock()
                .expect("tool traces lock")
                .push(std::mem::take(&mut self.sample));
        }
    }
}

#[derive(Clone, Default, Serialize)]
pub(crate) struct SessionSample {
    pub(crate) trace_id: String,
    pub(crate) dispatches: Vec<Dispatch>,
}

#[derive(Clone, Serialize)]
pub(crate) struct Dispatch {
    at_us: u128,
    pub(crate) tools: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct TrafficReport {
    tool_calls: u64,
    unbucketed_tool_calls: u64,
    window_ms: u64,
    peak_calls_per_window: u64,
    p95_calls_per_occupied_window: Option<u64>,
    pub(crate) session_samples: Vec<SessionSample>,
    calls_per_window: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn reports_calls_per_window_through_the_last_busy_window() {
        let traffic = Traffic::new(crate::runner::fixture::TRAFFIC);
        traffic.batch(3);
        tokio::time::advance(std::time::Duration::from_millis(250)).await;
        traffic.batch(2);
        let report = serde_json::to_value(traffic.report()).unwrap();
        assert_eq!(report["calls_per_window"], serde_json::json!([3, 0, 2]));
    }
}
