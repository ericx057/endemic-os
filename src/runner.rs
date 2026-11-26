use std::{
    future::{Future, poll_fn},
    pin::pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::Barrier, task::JoinSet};
use tokio_util::sync::CancellationToken;

mod faults;
#[cfg(test)]
pub(crate) mod fixture;
mod hotspots;
mod metrics;
mod pacing;
mod traffic;
pub use faults::{Fault, SessionFaults, StageFaults};
pub use metrics::{Criterion, Failure, Metrics, PassCriteria, StageResult};
pub use pacing::Pacing;
pub(crate) use traffic::{ToolTrace, TrafficShape};

pub async fn run_stage<F, Fut>(
    agents: usize,
    duration: Duration,
    drain: Duration,
    pacing: Pacing,
    cancel: CancellationToken,
    metrics: Arc<Metrics>,
    session: F,
) -> StageResult
where
    F: Fn(usize, Arc<Metrics>) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<(), Failure>> + Send + 'static,
{
    let ready = Arc::new(Barrier::new(agents + 1));
    let wave = pacing.sync_sessions.then(|| Arc::new(Barrier::new(agents)));
    let cutoff = CancellationToken::new();
    let mut workers = JoinSet::new();
    for agent in 0..agents {
        let (ready, wave, cutoff, metrics, session) = (
            ready.clone(),
            wave.clone(),
            cutoff.clone(),
            metrics.clone(),
            session.clone(),
        );
        workers.spawn(async move {
            ready.wait().await;
            if !pause(&cutoff, pacing.start_offset(agent)).await {
                return;
            }
            let _active = metrics.agent_started();
            for round in 0.. {
                if let Some(wave) = &wave {
                    tokio::select! {
                        biased;
                        _ = cutoff.cancelled() => return,
                        _ = wave.wait() => {}
                    }
                }
                if cutoff.is_cancelled() {
                    return;
                }
                metrics.session_started();
                let mut overhead = Duration::ZERO;
                let mut work = pin!(session(agent, metrics.clone()));
                let result = poll_fn(|cx| {
                    let start = Instant::now();
                    let result = work.as_mut().poll(cx);
                    overhead += start.elapsed();
                    result
                })
                .await;
                metrics.overhead(overhead);
                metrics.finished(result);
                if !pause(&cutoff, pacing.think(agent, round)).await {
                    return;
                }
                tokio::task::yield_now().await;
            }
        });
    }
    let held = tokio::select! {
        biased;
        _ = cancel.cancelled() => false,
        worker = workers.join_next() => {
            if worker.is_some() { metrics.finished(Err(Failure::Runner)); }
            false
        }
        _ = async { ready.wait().await; tokio::time::sleep(duration).await; } => true,
    };
    cutoff.cancel();
    if held {
        tokio::select! {
            _ = cancel.cancelled() => {},
            _ = tokio::time::timeout(drain, async {
                while let Some(result) = workers.join_next().await {
                    if result.is_err() { metrics.finished(Err(Failure::Runner)); }
                }
            }) => {},
        }
    }
    workers.shutdown().await;
    metrics.snapshot(agents, held && !cancel.is_cancelled())
}

async fn pause(cutoff: &CancellationToken, wait: Duration) -> bool {
    if wait.is_zero() {
        return true;
    }
    tokio::select! {
        biased;
        _ = cutoff.cancelled() => false,
        _ = tokio::time::sleep(wait) => true,
    }
}
