use super::*;
use crate::{
    config::tests::{assert_each_required, env_of},
    plan::Sessions,
    runner::{
        self, Failure, Pacing,
        fixture::{CRITERIA, DRAIN, TRAFFIC},
    },
};
use axum::{body::Body, http::Request};
use tower::ServiceExt;

const SETTINGS: [(&str, &str); 11] = [
    (
        "API_BEARER_TOKEN",
        "fixture-operator-token-0123456789abcdef",
    ),
    ("API_BODY_LIMIT_BYTES", "16384"),
    ("RUN_START_AGENTS", "2"),
    ("RUN_MAX_AGENTS", "8"),
    ("RUN_HOLD_SECONDS", "300"),
    ("RUN_DRAIN_SECONDS", "5"),
    ("STAGE_RUNNER_P95_MS", "50"),
    ("STAGE_RUNNER_P99_MS", "100"),
    ("STAGE_ERROR_BUDGET_PERCENT", "1"),
    ("TRAFFIC_WINDOW_MS", "100"),
    ("TRAFFIC_SESSION_SAMPLES", "16"),
];

impl Settings {
    /// Fixture settings with a short token the tests send as `Bearer api-secret`.
    pub(super) fn fixture() -> Self {
        Self {
            token: "api-secret".into(),
            ..Self::from_env(&env_of(&SETTINGS)).unwrap()
        }
    }

    /// Fixture settings for a short ramp from `start` to `max` agents.
    fn ramp(start: usize, max: usize) -> Self {
        Self {
            start_agents: start,
            max_agents: max,
            hold: Duration::from_secs(1),
            ..Self::fixture()
        }
    }
}

impl App {
    /// An app that skips planning: each run gets an empty plan and runs `session`.
    pub(super) fn fixture<F, Fut>(settings: Settings, session: F) -> Self
    where
        F: Fn(usize, Arc<Metrics>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), Failure>> + Send + 'static,
    {
        let session = Arc::new(session);
        Self::new(settings, move |request| {
            let session = session.clone();
            async move {
                let sessions: Sessions =
                    Arc::new(move |agent, metrics| Box::pin(session(agent, metrics)));
                let plan = Plan::new(&request);
                Ok(Prepared {
                    plan,
                    labels: Vec::new(),
                    sessions,
                })
            }
        })
    }
}

fn request() -> RunRequest {
    RunRequest::loopback("https://staging.example.com/mcp", None, None)
}

const BODY: &str = r#"{"target":{"url":"https://staging.example.com/mcp"},"credentials":{"bearer":"staging-token"}}"#;

async fn finished(app: &App, id: u64, within: Duration) -> RunReport {
    tokio::time::timeout(within, async {
        loop {
            let report = get_run(app, id).await.unwrap();
            if report.status != RunStatus::Running {
                break report;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[test]
fn every_api_setting_is_required_and_windows_cover_hold_plus_drain() {
    assert_each_required(&SETTINGS, Settings::from_env);
    let settings = Settings::from_env(&env_of(&SETTINGS)).unwrap();
    assert_eq!(settings.traffic.windows, TRAFFIC.windows);
    assert_eq!(settings.drain, DRAIN);
    let error = |name, value| {
        let mut pairs = SETTINGS.to_vec();
        pairs.retain(|(key, _)| *key != name);
        pairs.push((name, value));
        Settings::from_env(&env_of(&pairs))
            .err()
            .unwrap()
            .to_string()
    };
    assert_eq!(
        error("STAGE_ERROR_BUDGET_PERCENT", "100"),
        "STAGE_ERROR_BUDGET_PERCENT must be from 1 to 99"
    );
    assert_eq!(
        error("API_BEARER_TOKEN", "short"),
        "API_BEARER_TOKEN must be at least 32 visible ASCII bytes"
    );
    assert_eq!(
        error("RUN_START_AGENTS", "9"),
        "RUN_START_AGENTS must not exceed RUN_MAX_AGENTS"
    );
}

#[path = "tests/mcp_protocol.rs"]
mod mcp_protocol;

#[tokio::test(start_paused = true)]
async fn capacity_does_not_advance_past_an_inference_limited_stage() {
    let app = App::fixture(Settings::ramp(2, 8), |_, _| async {
        tokio::time::sleep(Duration::from_millis(15)).await;
        Err(Failure::InferenceThrottled)
    });
    let started = start_run(app.clone(), request()).await.unwrap();
    let report = finished(&app, started.id, Duration::from_secs(2)).await;
    assert!(report.status == RunStatus::FailedStage);
    assert_eq!(report.highest_passing_agents, None);
    assert_eq!(report.stages.len(), 1);
    assert!(!report.stages[0].passed);
    assert!(report.stages[0].result.inference_throttles > 0);
    assert_eq!(
        report.stages[0].failed_criterion,
        Some(runner::Criterion::SessionsCompleted)
    );
    let explanation = serde_json::to_value(&report.explanation).unwrap();
    assert_eq!(explanation["criterion"], "sessions_completed");
    assert_eq!(explanation["agents"], 2);
}

#[tokio::test(start_paused = true)]
async fn stage_counts_sessions_that_do_not_finish_during_drain() {
    let metrics = Arc::new(Metrics::default());
    let report = runner::run_stage(
        3,
        Duration::from_secs(1),
        DRAIN,
        Pacing::default(),
        CancellationToken::new(),
        metrics,
        |_, _| std::future::pending::<Result<(), Failure>>(),
    )
    .await;
    assert!(report.held);
    assert_eq!(report.started, 3);
    assert_eq!(report.interrupted(), 3);
    assert!(!report.passes(&CRITERIA));
}

#[tokio::test(start_paused = true)]
async fn capacity_reaches_ceiling_after_two_passing_stages() {
    let app = App::fixture(Settings::ramp(2, 3), |_, _| async {
        tokio::time::sleep(Duration::from_millis(15)).await;
        Ok(())
    });
    let report = start_run(app.clone(), request()).await.unwrap();
    let report = finished(&app, report.id, Duration::from_secs(4)).await;
    assert_eq!(report.plan.agents_per_stage, [2, 3]);
    assert!(report.status == RunStatus::ReachedCeiling);
    assert_eq!(report.highest_passing_agents, Some(3));
    assert_eq!(report.stages.len(), 2);
    assert!(
        report
            .stages
            .iter()
            .all(|stage| stage.passed && stage.interrupted == 0)
    );
}

#[tokio::test(start_paused = true)]
async fn paced_agents_start_spread_out_and_run_sessions_in_waves() {
    let sessions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = sessions.clone();
    let pacing = Pacing {
        start_spread_ms: 500,
        think_min_ms: 100,
        think_max_ms: 200,
        sync_sessions: true,
    };
    let report = runner::run_stage(
        4,
        Duration::from_secs(2),
        DRAIN,
        pacing,
        CancellationToken::new(),
        Arc::new(Metrics::default()),
        move |_, _| {
            counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok(())
            }
        },
    )
    .await;
    let sessions = sessions.load(std::sync::atomic::Ordering::Relaxed);
    assert!(report.passes(&CRITERIA));
    assert!(sessions >= 8 && sessions.is_multiple_of(4), "{sessions}");
}

#[tokio::test(start_paused = true)]
async fn a_held_run_is_one_stage_at_the_requested_agents() {
    let app = App::fixture(Settings::ramp(2, 8), |_, _| async {
        tokio::time::sleep(Duration::from_millis(15)).await;
        Ok(())
    });
    let held = RunRequest {
        hold_agents: Some(5),
        ..request()
    };
    let report = start_run(app.clone(), held).await.unwrap();
    assert_eq!(report.plan.hold_agents, Some(5));
    let report = finished(&app, report.id, Duration::from_secs(4)).await;
    assert_eq!(report.plan.agents_per_stage, [5]);
    assert!(report.status == RunStatus::Completed);
    assert_eq!(report.highest_passing_agents, Some(5));

    let over = RunRequest {
        hold_agents: Some(9),
        ..request()
    };
    let (status, error) = start_run(app, over).await.err().unwrap();
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error, "hold_agents must be from 1 to 8");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "local performance measurement"]
async fn benchmark_runner() {
    let agents: usize = std::env::var("BENCH_AGENTS")
        .expect("set BENCH_AGENTS to the requested fixture concurrency")
        .parse()
        .expect("BENCH_AGENTS must be a positive integer");
    assert!(
        (1..usize::MAX).contains(&agents),
        "BENCH_AGENTS is outside the supported range"
    );
    let report = runner::run_stage(
        agents,
        Duration::from_secs(10),
        DRAIN,
        Pacing::default(),
        CancellationToken::new(),
        Arc::new(Metrics::default()),
        |_, _| async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(())
        },
    )
    .await;
    println!("{}", serde_json::to_string(&report).unwrap());
    assert_eq!(report.peak_active, agents as u64);
    assert!(report.passes(&CRITERIA));
}

#[tokio::test]
async fn api_starts_only_one_run_and_accepts_cancellation() {
    let app = router(App::fixture(Settings::fixture(), |_, _| {
        std::future::pending::<Result<(), Failure>>()
    }));
    let post = || {
        Request::post("/runs")
            .header("authorization", "Bearer api-secret")
            .header("content-type", "application/json")
            .body(Body::from(BODY))
            .unwrap()
    };
    let item = |method| {
        Request::builder()
            .method(method)
            .uri("/runs/1")
            .header("authorization", "Bearer api-secret")
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(post()).await.unwrap().status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        app.clone().oneshot(post()).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.clone()
            .oneshot(item(axum::http::Method::DELETE))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let response = app
                .clone()
                .oneshot(item(axum::http::Method::GET))
                .await
                .unwrap();
            let bytes = axum::body::to_bytes(response.into_body(), 16_384)
                .await
                .unwrap();
            let report: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            if report["status"] == "cancelled" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn capacity_needs_samples_and_full_concurrency() {
    let metrics = Metrics::default();
    assert!(!metrics.snapshot(1, true).passes(&CRITERIA));
    metrics.overhead(Duration::ZERO);
    metrics.finished(Ok(()));
    assert!(!metrics.snapshot(1, false).passes(&CRITERIA));
}

#[tokio::test]
async fn stage_runs_exact_concurrency_and_drops_in_flight_sessions() {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let cancel = CancellationToken::new();
    let metrics = Arc::new(Metrics::default());
    struct Active(Arc<AtomicUsize>);
    impl Drop for Active {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Relaxed);
        }
    }
    let stage = tokio::spawn(runner::run_stage(
        8,
        Duration::from_secs(5),
        DRAIN,
        Pacing::default(),
        cancel.clone(),
        metrics,
        {
            let active = active.clone();
            let peak = peak.clone();
            move |_, _| {
                let active = active.clone();
                let peak = peak.clone();
                async move {
                    peak.fetch_max(active.fetch_add(1, Relaxed) + 1, Relaxed);
                    let _active = Active(active);
                    std::future::pending::<Result<(), Failure>>().await
                }
            }
        },
    ));
    tokio::time::timeout(Duration::from_secs(1), async {
        while active.load(Relaxed) != 8 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    let report: StageResult = stage.await.unwrap();
    assert!(!report.held);
    assert_eq!(report.peak_active, 8);
    assert_eq!(report.interrupted(), 8);
    assert_eq!(peak.load(Relaxed), 8);
    assert_eq!(active.load(Relaxed), 0);
}

#[tokio::test]
async fn api_passes_auth_and_missing_run_status() {
    let app = router(App::fixture(Settings::fixture(), |_, _| async { Ok(()) }));
    let request = |auth: Option<&str>| {
        let mut request = Request::builder().uri("/runs/999");
        if let Some(value) = auth {
            request = request.header("authorization", value);
        }
        request.body(Body::empty()).unwrap()
    };
    assert_eq!(
        app.clone().oneshot(request(None)).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.oneshot(request(Some("Bearer api-secret")))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn post_runs_rejects_invalid_bodies_with_a_message_and_no_token() {
    let app = router(App::fixture(Settings::fixture(), |_, _| async { Ok(()) }));
    for (body, expected) in [
        (
            r#"{"mode":"fixed","agents":2,"hold_seconds":60}"#,
            "invalid run request: unknown field `mode`",
        ),
        (
            r#"{"target":{"url":"http://s.example.com"},"credentials":{"bearer":"staging-token"}}"#,
            "target.url must be an HTTPS URL",
        ),
        (
            r#"{"target":{"url":"https://s.example.com"},"credentials":"staging-token"}"#,
            "invalid run request at line 1",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/runs")
                    .header("authorization", "Bearer api-secret")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(response.into_body(), 16_384)
            .await
            .unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!text.contains("staging-token"), "{text}");
        let error: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            error["error"].as_str().unwrap().starts_with(expected),
            "{text}"
        );
    }
}

#[tokio::test]
async fn a_run_that_fails_before_stages_reports_failed_with_its_plan() {
    let app = App::new(Settings::fixture(), |request| async move {
        let mut plan = Plan::new(&request);
        plan.tools = vec!["search".into()];
        let error = "task generation failed: fixture".to_owned();
        Err(Failed { plan, error })
    });
    let started = start_run(app.clone(), request()).await.unwrap();
    let report = serde_json::to_value(finished(&app, started.id, Duration::from_secs(1)).await);
    assert_eq!(
        report.unwrap(),
        serde_json::json!({"id":1,"status":"failed","highest_passing_agents":null,
            "plan":{"target_kind":"mcp","tools":["search"],"parallel_tools":[],"tasks":[],
                "hold_agents":null,"pacing":{"start_spread_ms":0,"think_min_ms":0,"think_max_ms":0,"sync_sessions":false},"faults":[],"agents_per_stage":[]},
            "error":"task generation failed: fixture","stages":[],"explanation":null})
    );
}

#[tokio::test]
async fn run_status_revalidates_with_its_etag() {
    let app = App::new(Settings::fixture(), |request| async move {
        let plan = Plan::new(&request);
        let error = "task generation failed: fixture".to_owned();
        Err(Failed { plan, error })
    });
    let started = start_run(app.clone(), request()).await.unwrap();
    finished(&app, started.id, Duration::from_secs(1)).await;
    let get = |etag: Option<&str>| {
        let mut request = Request::builder()
            .uri(format!("/runs/{}", started.id))
            .header("authorization", "Bearer api-secret");
        if let Some(etag) = etag {
            request = request.header("if-none-match", etag);
        }
        router(app.clone()).oneshot(request.body(Body::empty()).unwrap())
    };
    let response = get(None).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-cache");
    let etag = response.headers()["etag"].to_str().unwrap().to_owned();
    let response = get(Some(&etag)).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(response.headers()["etag"], etag.as_str());
    let body = axum::body::to_bytes(response.into_body(), 16)
        .await
        .unwrap();
    assert!(body.is_empty());
    assert_eq!(
        get(Some("\"stale\"")).await.unwrap().status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn cancelling_during_planning_stops_the_run() {
    let app = App::new(Settings::fixture(), |_| std::future::pending());
    let started = start_run(app.clone(), request()).await.unwrap();
    cancel_run(&app, started.id).await.unwrap();
    let report = finished(&app, started.id, Duration::from_secs(1)).await;
    assert!(report.status == RunStatus::Cancelled);
    assert!(report.stages.is_empty());
}

#[path = "tests/managed_run.rs"]
mod managed_run;
