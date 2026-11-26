use super::*;
use tokio::time::Instant;

const TIMEOUT: Duration = Duration::from_secs(2);

fn fault(tool: &str, percent: u8, effect: Effect) -> Fault {
    Fault {
        tool: tool.into(),
        first_stage: 2,
        last_stage: 3,
        percent,
        effect,
    }
}

fn metrics(faults: &[Fault], stage: usize) -> Metrics {
    Metrics::default().with_faults(StageFaults::new(7, stage, faults))
}

#[test]
fn checks_shape_then_plan_tools_and_timeout() {
    let latency = |delay_ms| fault("search", 50, Effect::Latency { delay_ms });
    assert!(latency(10).check().is_ok());
    assert!(latency(0).check().is_err());
    assert!(fault("search", 0, Effect::Error).check().is_err());
    assert!(fault("search", 101, Effect::Error).check().is_err());
    let backwards = Fault {
        first_stage: 4,
        ..fault("search", 50, Effect::Error)
    };
    assert!(backwards.check().is_err());
    let unnumbered = Fault {
        first_stage: 0,
        ..fault("search", 50, Effect::Error)
    };
    assert!(unnumbered.check().is_err());

    let tools = ["search".to_owned()];
    assert!(latency(1999).fits(&tools, TIMEOUT).is_ok());
    assert!(latency(2000).fits(&tools, TIMEOUT).is_err());
    assert!(
        fault("refund", 50, Effect::Error)
            .fits(&tools, TIMEOUT)
            .is_err()
    );
}

#[test]
fn only_faults_in_the_stage_window_are_active() {
    let faults = [fault("search", 100, Effect::Error)];
    assert!(StageFaults::new(7, 1, &faults).faults().is_empty());
    assert_eq!(StageFaults::new(7, 3, &faults).faults(), &faults);
    assert!(StageFaults::new(7, 4, &faults).faults().is_empty());
}

#[tokio::test(start_paused = true)]
async fn draws_are_reproducible_and_counted_apart_from_the_target() {
    let faults = [fault("search", 30, Effect::Error)];
    let first = metrics(&faults, 2);
    let second = metrics(&faults, 2);
    let hits = outcomes(&first).await;
    assert_eq!(hits, outcomes(&second).await);
    let injected = hits.iter().filter(|hit| **hit).count() as u64;
    assert!((30..90).contains(&injected), "{injected} of 200 calls");
    let result = first.snapshot(1, true);
    assert_eq!(result.injected["search"].errors, injected);
    assert_eq!((result.target_errors, result.target.samples), (0, 0));
}

#[tokio::test(start_paused = true)]
async fn latency_delays_and_timeouts_wait_then_fail() {
    let faults = [
        fault("slow", 100, Effect::Latency { delay_ms: 300 }),
        fault("hung", 100, Effect::Timeout),
    ];
    let metrics = metrics(&faults, 2);
    let mut session = SessionFaults::new(0, 0, TIMEOUT);
    let start = Instant::now();
    assert!(
        session
            .inject(&metrics, ["slow", "other"].into_iter())
            .await
            .is_ok()
    );
    assert_eq!(start.elapsed(), Duration::from_millis(300));
    let start = Instant::now();
    let hung = session.inject(&metrics, ["hung"].into_iter()).await;
    assert_eq!(hung, Err(Failure::Injected));
    assert_eq!(start.elapsed(), TIMEOUT);
    let injected = metrics.snapshot(1, true).injected;
    assert_eq!(
        (injected["slow"].delayed, injected["hung"].timeouts),
        (1, 1)
    );
}

async fn outcomes(metrics: &Metrics) -> Vec<bool> {
    let mut results = Vec::new();
    for agent in 0..20 {
        let mut session = SessionFaults::new(agent, 0, TIMEOUT);
        for _ in 0..10 {
            let result = session.inject(metrics, ["search"].into_iter()).await;
            results.push(result.is_err());
        }
    }
    results
}

#[test]
fn injected_failures_are_outside_the_error_budget_and_not_interrupted() {
    let metrics = Metrics::default();
    let _active = metrics.agent_started();
    for _ in 0..10 {
        metrics.session_started();
        metrics.overhead(Duration::from_millis(5));
        metrics.finished(Ok(()));
    }
    metrics.session_started();
    metrics.finished(Err(Failure::Injected));
    let result = metrics.snapshot(1, true);
    assert_eq!((result.injected_failures, result.interrupted()), (1, 0));
    assert!(result.passes(&crate::runner::fixture::CRITERIA));
}
