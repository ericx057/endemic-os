use crate::{
    config::{self, Env},
    plan::{Failed, Plan, Prepared},
    request::RunRequest,
    runner::{
        Criterion, Fault, Metrics, PassCriteria, StageFaults, StageResult, TrafficShape, run_stage,
    },
};
use anyhow::Context;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, ETAG, IF_NONE_MATCH},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use serde_json::json;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[path = "mcp_api.rs"]
mod mcp_api;

type Prepare = Arc<
    dyn Fn(RunRequest) -> Pin<Box<dyn Future<Output = Result<Prepared, Failed>> + Send>>
        + Send
        + Sync,
>;

pub struct Settings {
    token: String,
    body_limit: usize,
    start_agents: usize,
    max_agents: usize,
    hold: Duration,
    drain: Duration,
    criteria: PassCriteria,
    traffic: TrafficShape,
}

impl Settings {
    pub fn from_env(env: Env) -> anyhow::Result<Self> {
        let token = config::required(env, "API_BEARER_TOKEN")?;
        anyhow::ensure!(
            token.len() >= 32 && token.bytes().all(|b| b.is_ascii_graphic()),
            "API_BEARER_TOKEN must be at least 32 visible ASCII bytes"
        );
        let start_agents = config::positive(env, "RUN_START_AGENTS")?;
        let max_agents = config::positive(env, "RUN_MAX_AGENTS")?;
        anyhow::ensure!(
            start_agents <= max_agents,
            "RUN_START_AGENTS must not exceed RUN_MAX_AGENTS"
        );
        let hold_seconds = config::positive(env, "RUN_HOLD_SECONDS")?;
        let drain = config::seconds(env, "RUN_DRAIN_SECONDS")?;
        let window_ms: u64 = config::positive(env, "TRAFFIC_WINDOW_MS")?;
        let error_budget_percent = config::positive(env, "STAGE_ERROR_BUDGET_PERCENT")?;
        anyhow::ensure!(
            error_budget_percent < 100,
            "STAGE_ERROR_BUDGET_PERCENT must be from 1 to 99"
        );
        Ok(Self {
            token,
            body_limit: config::positive(env, "API_BODY_LIMIT_BYTES")?,
            start_agents,
            max_agents,
            hold: Duration::from_secs(hold_seconds),
            drain,
            criteria: PassCriteria {
                runner_p95_ms: config::positive(env, "STAGE_RUNNER_P95_MS")?,
                runner_p99_ms: config::positive(env, "STAGE_RUNNER_P99_MS")?,
                error_budget_percent,
            },
            traffic: TrafficShape {
                window_ms,
                windows: windows(hold_seconds, drain.as_secs(), window_ms)?,
                session_samples: config::positive(env, "TRAFFIC_SESSION_SAMPLES")?,
            },
        })
    }
}

fn windows(hold_seconds: u64, drain_seconds: u64, window_ms: u64) -> anyhow::Result<usize> {
    hold_seconds
        .checked_add(drain_seconds)
        .and_then(|seconds| seconds.checked_mul(1000))
        .and_then(|ms| usize::try_from(ms.div_ceil(window_ms) + 1).ok())
        .context("RUN_HOLD_SECONDS plus RUN_DRAIN_SECONDS is too large")
}

#[derive(Clone)]
pub struct App {
    settings: Arc<Settings>,
    run: Arc<Mutex<Option<Run>>>,
    prepare: Prepare,
}

impl App {
    pub fn new<F, Fut>(settings: Settings, prepare: F) -> Self
    where
        F: Fn(RunRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Prepared, Failed>> + Send + 'static,
    {
        Self {
            settings: Arc::new(settings),
            run: Arc::new(Mutex::new(None)),
            prepare: Arc::new(move |request| Box::pin(prepare(request))),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RunStatus {
    Running,
    Cancelled,
    Failed,
    FailedStage,
    ReachedCeiling,
    Completed,
}

#[derive(Clone, Serialize)]
struct RunReport {
    id: u64,
    status: RunStatus,
    highest_passing_agents: Option<usize>,
    plan: Plan,
    error: Option<String>,
    stages: Vec<Arc<StageReport>>,
    explanation: Option<explain::Explanation>,
}

impl RunReport {
    fn etag(&self) -> String {
        format!(
            "\"{}-{}-{}-{}-{}\"",
            self.id,
            self.status as u8,
            self.plan.tasks.len(),
            self.plan.agents_per_stage.len(),
            self.stages.len()
        )
    }
}

#[derive(Clone, Serialize)]
struct StageReport {
    #[serde(flatten)]
    result: StageResult,
    faults: Vec<Fault>,
    hold_seconds: u64,
    passed: bool,
    failed_criterion: Option<Criterion>,
    interrupted: u64,
}

struct Run {
    report: RunReport,
    cancel: CancellationToken,
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/runs", post(start))
        .route("/runs/{id}", get(report).delete(cancel))
        .route("/mcp", post(mcp_api::handle))
        .route_layer(middleware::from_fn_with_state(app.clone(), authenticate))
        .layer(DefaultBodyLimit::max(app.settings.body_limit))
        .route("/health", get(|| async { StatusCode::NO_CONTENT }))
        .with_state(app)
}

async fn authenticate(State(app): State<App>, request: Request, next: Next) -> Response {
    if request.uri().path() == "/mcp" && request.headers().contains_key(axum::http::header::ORIGIN)
    {
        return axum::response::IntoResponse::into_response(StatusCode::FORBIDDEN);
    }
    let token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.as_bytes().strip_prefix(b"Bearer "));
    if token.is_some_and(|value| bool::from(app.settings.token.as_bytes().ct_eq(value))) {
        return next.run(request).await;
    }
    axum::response::IntoResponse::into_response((
        StatusCode::UNAUTHORIZED,
        [(axum::http::header::WWW_AUTHENTICATE, "Bearer")],
    ))
}

async fn start(State(app): State<App>, body: Bytes) -> Response {
    let started = match RunRequest::parse(&body) {
        Ok(request) => start_run(app, request).await,
        Err(error) => Err((StatusCode::UNPROCESSABLE_ENTITY, error)),
    };
    match started {
        Ok(report) => (StatusCode::ACCEPTED, Json(report)).into_response(),
        Err((status, error)) => (status, Json(json!({"error": error}))).into_response(),
    }
}

async fn start_run(app: App, request: RunRequest) -> Result<RunReport, (StatusCode, String)> {
    if request
        .hold_agents
        .is_some_and(|agents| agents > app.settings.max_agents)
    {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("hold_agents must be from 1 to {}", app.settings.max_agents),
        ));
    }
    request
        .pacing
        .validate(app.settings.hold)
        .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    let mut run = app.run.lock().await;
    if run
        .as_ref()
        .is_some_and(|run| run.report.status == RunStatus::Running)
    {
        return Err((StatusCode::CONFLICT, "a run is already in progress".into()));
    }
    let report = RunReport {
        id: run.as_ref().map_or(1, |run| run.report.id + 1),
        status: RunStatus::Running,
        highest_passing_agents: None,
        plan: Plan::new(&request),
        error: None,
        stages: Vec::new(),
        explanation: None,
    };
    let cancel = CancellationToken::new();
    *run = Some(Run {
        report: report.clone(),
        cancel: cancel.clone(),
    });
    drop(run);
    tokio::spawn(execute(app, request, cancel));
    Ok(report)
}

async fn report(State(app): State<App>, Path(id): Path<u64>, headers: HeaderMap) -> Response {
    let run = app.run.lock().await;
    let Some(report) = run
        .as_ref()
        .map(|run| &run.report)
        .filter(|report| report.id == id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let etag = report.etag();
    let fresh = headers
        .get(IF_NONE_MATCH)
        .is_some_and(|tag| tag.as_bytes() == etag.as_bytes());
    let report = (!fresh).then(|| report.clone());
    drop(run);
    let headers = [(ETAG, etag), (CACHE_CONTROL, "no-cache".to_owned())];
    match report {
        Some(report) => (headers, Json(report)).into_response(),
        None => (StatusCode::NOT_MODIFIED, headers).into_response(),
    }
}

async fn get_run(app: &App, id: u64) -> Result<RunReport, StatusCode> {
    app.run
        .lock()
        .await
        .as_ref()
        .filter(|run| run.report.id == id)
        .map(|run| run.report.clone())
        .ok_or(StatusCode::NOT_FOUND)
}

async fn cancel(State(app): State<App>, Path(id): Path<u64>) -> Result<StatusCode, StatusCode> {
    cancel_run(&app, id).await.map(|()| StatusCode::ACCEPTED)
}

async fn cancel_run(app: &App, id: u64) -> Result<(), StatusCode> {
    let run = app.run.lock().await;
    let run = run
        .as_ref()
        .filter(|run| run.report.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    if run.report.status != RunStatus::Running {
        return Err(StatusCode::CONFLICT);
    }
    run.cancel.cancel();
    Ok(())
}

async fn execute(app: App, request: RunRequest, cancel: CancellationToken) {
    let (hold_agents, pacing) = (request.hold_agents, request.pacing);
    let faults = request.faults.clone();
    let prepared = tokio::select! {
        biased;
        _ = cancel.cancelled() => None,
        prepared = (app.prepare)(request) => Some(prepared),
    };
    let (id, labels, sessions) = {
        let mut run = app.run.lock().await;
        let Some(run) = run.as_mut() else { return };
        match prepared {
            None => {
                run.report.status = RunStatus::Cancelled;
                return;
            }
            Some(Err(Failed { plan, error })) => {
                run.report.plan = plan;
                run.report.error = Some(error);
                run.report.status = RunStatus::Failed;
                return;
            }
            Some(Ok(Prepared {
                plan,
                labels,
                sessions,
            })) => {
                run.report.plan = plan;
                (run.report.id, labels, sessions)
            }
        }
    };
    let settings = &app.settings;
    let mut agents = hold_agents.unwrap_or(settings.start_agents);
    for stage in 1.. {
        let stage_faults = StageFaults::new(id, stage, &faults);
        if let Some(run) = app.run.lock().await.as_mut() {
            run.report.plan.agents_per_stage.push(agents);
        }
        let sessions = sessions.clone();
        let result = run_stage(
            agents,
            settings.hold,
            settings.drain,
            pacing,
            cancel.clone(),
            Arc::new(Metrics::new(settings.traffic, &labels).with_faults(stage_faults.clone())),
            move |agent, metrics| sessions(agent, metrics),
        )
        .await;
        let failed_criterion = result.failed_criterion(&settings.criteria);
        let passed = failed_criterion.is_none();
        let held = result.held;
        let mut run = app.run.lock().await;
        let Some(run) = run.as_mut() else { return };
        run.report.stages.push(Arc::new(StageReport {
            hold_seconds: settings.hold.as_secs(),
            interrupted: result.interrupted(),
            result,
            faults: stage_faults.faults().to_vec(),
            passed,
            failed_criterion,
        }));
        if passed {
            run.report.highest_passing_agents = Some(agents);
        }
        run.report.status = if cancel.is_cancelled() {
            RunStatus::Cancelled
        } else if !held || !passed {
            RunStatus::FailedStage
        } else if hold_agents.is_some() {
            RunStatus::Completed
        } else if agents == settings.max_agents {
            RunStatus::ReachedCeiling
        } else {
            RunStatus::Running
        };
        if run.report.status == RunStatus::FailedStage {
            run.report.explanation = explain::explain(&run.report.stages);
        }
        if run.report.status != RunStatus::Running {
            return;
        }
        agents = (agents * 2).min(settings.max_agents);
    }
}

#[path = "explain.rs"]
mod explain;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
