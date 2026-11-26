//! The seam between the engines and the system under test: one variant per target kind.

use crate::{
    http_api, mcp,
    runner::{Failure, Metrics, ToolTrace},
};
use reqwest::header::HeaderValue;
use serde_json::Value;
use std::{
    future::Future,
    hash::{BuildHasher, RandomState},
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Instant,
};

#[derive(Clone)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub schema: Value,
    /// Safe to dispatch in parallel: HTTP GET or HEAD, or MCP `readOnlyHint: true`.
    pub read_only: bool,
}

pub enum Target {
    Mcp(mcp::Target),
    Http(http_api::Target),
}

pub enum Session<'a> {
    Mcp(mcp::Session<'a>),
    Http(http_api::Session<'a>),
}

/// HTTP settings every target kind shares; the client is the run's pinned one (`egress`).
pub(crate) struct Wire {
    pub(crate) client: reqwest::Client,
    /// Largest request body a call may send.
    pub(crate) request_limit: usize,
    /// Largest response body a call may receive.
    pub(crate) response_limit: usize,
}

impl Target {
    pub async fn connect(&self) -> Result<Session<'_>, Failure> {
        match self {
            Self::Mcp(target) => target.connect().await.map(Session::Mcp),
            Self::Http(target) => target.connect().await.map(Session::Http),
        }
    }
}

impl<'a> Session<'a> {
    pub fn tools(&self) -> &[Tool] {
        match self {
            Self::Mcp(session) => session.tools(),
            Self::Http(session) => session.tools(),
        }
    }

    pub fn observe(&mut self, metrics: &'a Metrics) {
        match self {
            Self::Mcp(session) => session.observe(metrics),
            Self::Http(session) => session.observe(metrics),
        }
    }

    pub fn can_parallel(&self, name: &str) -> bool {
        match self {
            Self::Mcp(session) => session.can_parallel(name),
            Self::Http(session) => session.can_parallel(name),
        }
    }

    /// Sends one model batch; see `check_batch` for which batches are allowed.
    pub async fn call_group(
        &mut self,
        calls: Vec<(String, Value)>,
        metrics: &Metrics,
    ) -> Result<Vec<Value>, Failure> {
        match self {
            Self::Mcp(session) => session.call_group(calls, metrics).await,
            Self::Http(session) => session.call_group(calls, metrics).await,
        }
    }

    pub async fn close(self) -> Result<(), Failure> {
        match self {
            Self::Mcp(session) => session.close().await,
            Self::Http(session) => session.close().await,
        }
    }
}

/// A batch is one call, or several calls that are all approved for parallel dispatch.
pub(crate) fn check_batch(
    calls: &[(String, Value)],
    can_parallel: impl Fn(&str) -> bool,
) -> Result<(), Failure> {
    let serial = calls.len() == 1;
    if calls.is_empty() || !(serial || calls.iter().all(|(name, _)| can_parallel(name))) {
        return Err(Failure::Runner);
    }
    Ok(())
}

/// Counts the batch and adds it to the session's sampled trace.
pub(crate) fn record_batch<'n>(
    metrics: &Metrics,
    trace: &mut Option<ToolTrace<'_>>,
    names: impl ExactSizeIterator<Item = &'n str>,
) {
    metrics.tool_batch(names.len());
    if let Some(trace) = trace {
        trace.record(names);
    }
}

/// Sends prepared calls concurrently, timing each into the hotspot row named by its label.
/// Results keep call order.
pub(crate) async fn dispatch<F>(
    metrics: &Metrics,
    calls: impl Iterator<Item = (String, F)>,
) -> Result<Vec<Value>, Failure>
where
    F: Future<Output = Result<Value, Failure>>,
{
    let sends = calls.map(|(label, send)| async move {
        let start = Instant::now();
        let result = send.await;
        metrics.tool_call(&label, start.elapsed(), result.is_ok());
        result
    });
    futures_util::future::join_all(sends)
        .await
        .into_iter()
        .collect()
}

/// W3C trace context for one agent session: every request shares the trace id
/// and gets a fresh span id, so the target's own tracing groups work by session.
pub struct TraceContext {
    trace_id: String,
}

impl TraceContext {
    pub fn new() -> Self {
        Self {
            trace_id: format!("{:016x}{:016x}", nonzero_id(), nonzero_id()),
        }
    }

    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    pub fn traceparent(&self) -> HeaderValue {
        // W3C Trace Context: version 00, trace id, parent span id, flags 01 (sampled).
        let value = format!("00-{}-{:016x}-01", self.trace_id, nonzero_id());
        HeaderValue::from_str(&value).expect("hex digits are a valid header value")
    }
}

/// Trace and span ids need uniqueness, not secrecy, so std's randomly keyed
/// hasher over a process-wide counter and the clock stands in for a rand dependency.
fn nonzero_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    loop {
        let id = RandomState::new().hash_one((COUNTER.fetch_add(1, Relaxed), Instant::now()));
        if id != 0 {
            return id;
        }
    }
}

#[cfg(test)]
mod tests;
