use crate::{
    http_api, mcp,
    runner::{Failure, Metrics, SessionFaults, ToolTrace},
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

pub(crate) struct Wire {
    pub(crate) client: reqwest::Client,
    pub(crate) request_limit: usize,
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

    pub async fn call_group(
        &mut self,
        calls: Vec<(String, Value)>,
        metrics: &Metrics,
        faults: &mut SessionFaults,
    ) -> Result<Vec<Value>, Failure> {
        check_batch(&calls, |name| self.can_parallel(name))?;
        let tools = calls.iter().map(|(name, _)| name.as_str());
        faults.inject(metrics, tools).await?;
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
        let value = format!("00-{}-{:016x}-01", self.trace_id, nonzero_id());
        HeaderValue::from_str(&value).expect("hex digits are a valid header value")
    }
}

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
