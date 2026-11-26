//! Managed-run planning: from the user's request to the run's tools, parallel set, tasks, and
//! agent sessions. Endemic decides all of it; the report records it under `plan`.

use crate::{
    config::{self, Env},
    egress::Egress,
    http_api, mcp,
    open_weight::{Engine, model_tools},
    request::RunRequest,
    runner::{Failure, Metrics, Pacing},
    target::{Target, Tool, Wire},
};
use reqwest::{
    Url,
    header::{ACCEPT, AUTHORIZATION},
};
use serde::Serialize;
use serde_json::{Value, value::RawValue};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

pub type Session = Pin<Box<dyn Future<Output = Result<(), Failure>> + Send>>;
/// Starts one agent session from the agent's index and the stage's metrics.
pub type Sessions = Arc<dyn Fn(usize, Arc<Metrics>) -> Session + Send + Sync>;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetKind {
    Mcp,
    Http,
}

/// What Endemic decided; fields not determined yet (or ever, for a failed run) stay empty.
#[derive(Clone, Serialize)]
pub struct Plan {
    pub target_kind: TargetKind,
    pub tools: Vec<String>,
    pub parallel_tools: Vec<String>,
    pub tasks: Vec<String>,
    pub hold_agents: Option<usize>,
    pub pacing: Pacing,
    pub agents_per_stage: Vec<usize>,
}

impl Plan {
    pub fn new(request: &RunRequest) -> Self {
        Self {
            target_kind: match request.openapi_url {
                Some(_) => TargetKind::Http,
                None => TargetKind::Mcp,
            },
            tools: Vec::new(),
            parallel_tools: Vec::new(),
            tasks: Vec::new(),
            hold_agents: request.hold_agents,
            pacing: request.pacing,
            agents_per_stage: Vec::new(),
        }
    }
}

pub struct Prepared {
    pub plan: Plan,
    pub labels: Vec<String>,
    pub sessions: Sessions,
}

pub struct Failed {
    pub plan: Plan,
    pub error: String,
}

pub struct Planner {
    engine: Arc<Engine>,
    egress: Egress,
    connect: Duration,
    timeout: Duration,
    request_limit: usize,
    response_limit: usize,
    spec_limit: usize,
    max_tools: usize,
    task_count: usize,
}

impl Planner {
    pub fn from_env(env: Env, engine: Arc<Engine>) -> anyhow::Result<Self> {
        Ok(Self {
            engine,
            egress: Egress::new(),
            connect: config::seconds(env, "TARGET_CONNECT_TIMEOUT_SECONDS")?,
            timeout: config::seconds(env, "TARGET_TIMEOUT_SECONDS")?,
            request_limit: config::positive(env, "TARGET_REQUEST_LIMIT_BYTES")?,
            response_limit: config::positive(env, "TARGET_RESPONSE_LIMIT_BYTES")?,
            spec_limit: config::positive(env, "OPENAPI_SPEC_LIMIT_BYTES")?,
            max_tools: config::positive(env, "RUN_MAX_TOOLS")?,
            task_count: config::positive(env, "PLAN_TASK_COUNT")?,
        })
    }

    #[allow(clippy::result_large_err)]
    pub async fn prepare(&self, request: RunRequest) -> Result<Prepared, Failed> {
        let mut plan = Plan::new(&request);
        let (target, tools) = match self.target(&request).await {
            Ok(found) => found,
            Err(error) => return Err(Failed { plan, error }),
        };
        let names = |read_only_only: bool| {
            let chosen = tools
                .iter()
                .filter(|tool| tool.read_only || !read_only_only);
            chosen.map(|tool| tool.name.clone()).collect()
        };
        plan.tools = names(false);
        plan.parallel_tools = names(true);
        // more tools than RUN_MAX_TOOLS fails the run; there is no selection heuristic.
        if tools.is_empty() || tools.len() > self.max_tools {
            let error = format!(
                "the target exposes {} tools; a run needs 1 to {} (RUN_MAX_TOOLS)",
                tools.len(),
                self.max_tools
            );
            return Err(Failed { plan, error });
        }
        let model_tools = match model_tools(&tools) {
            Ok(model_tools) => model_tools,
            Err(error) => return Err(Failed { plan, error }),
        };
        let use_case = request.use_case.as_deref();
        match self.engine.tasks(&tools, use_case, self.task_count).await {
            Ok(tasks) => plan.tasks = tasks,
            Err(error) => return Err(Failed { plan, error }),
        }
        let labels = match &target {
            Target::Mcp(_) => plan.tools.clone(),
            Target::Http(target) => target.labels(),
        };
        let sessions = sessions(self.engine.clone(), target, model_tools, plan.tasks.clone());
        Ok(Prepared {
            plan,
            labels,
            sessions,
        })
    }

    async fn target(&self, request: &RunRequest) -> Result<(Target, Vec<Tool>), String> {
        let urls: Vec<Url> = std::iter::once(request.url.clone())
            .chain(request.openapi_url.clone())
            .collect();
        let wire = Wire {
            client: (self.egress.client(&urls, self.connect, self.timeout)).await?,
            request_limit: self.request_limit,
            response_limit: self.response_limit,
        };
        let auth = request.bearer.header();
        let Some(spec_url) = &request.openapi_url else {
            let target = mcp::Target::new(request.url.clone(), auth, wire);
            let (target, tools) = target.discover().await.map_err(|_| {
                "MCP tool discovery failed: the target refused the connection or the token, \
                 or answered with an invalid MCP response"
                    .to_owned()
            })?;
            return Ok((Target::Mcp(target), tools));
        };
        let spec = self.spec(&wire.client, spec_url, request).await?;
        let target = http_api::Target::new(request.url.clone(), auth, &spec, wire)
            .map_err(|error| format!("the OpenAPI spec cannot be used: {error:#}"))?;
        let tools = target.tools().to_vec();
        Ok((Target::Http(target), tools))
    }

    async fn spec(
        &self,
        client: &reqwest::Client,
        url: &Url,
        request: &RunRequest,
    ) -> Result<Value, String> {
        let fail = |what: String| format!("the OpenAPI spec at {url} {what}");
        let mut get = client.get(url.clone()).header(ACCEPT, "application/json");
        // The token goes only to the target's own origin; a spec hosted elsewhere gets none.
        if url.origin() == request.url.origin() {
            get = get.header(AUTHORIZATION, request.bearer.header());
        }
        let mut response = get
            .send()
            .await
            .map_err(|_| fail("cannot be fetched".into()))?;
        if !response.status().is_success() {
            return Err(fail(format!(
                "answered HTTP {}",
                response.status().as_u16()
            )));
        }
        let mut body = Vec::new();
        while let Some(chunk) = (response.chunk().await).map_err(|_| fail("broke off".into()))? {
            if body.len() + chunk.len() > self.spec_limit {
                let limit = self.spec_limit;
                return Err(fail(format!(
                    "exceeds OPENAPI_SPEC_LIMIT_BYTES ({limit} bytes)"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| fail("is not JSON".into()))
    }
}

/// Round-robin: agent `i` pursues task `i % tasks.len()` in every stage.
fn task(tasks: &[String], agent: usize) -> &str {
    &tasks[agent % tasks.len()]
}

fn sessions(
    engine: Arc<Engine>,
    target: Target,
    tools: Arc<RawValue>,
    tasks: Vec<String>,
) -> Sessions {
    let target = Arc::new(target);
    let tasks: Arc<[String]> = tasks.into();
    Arc::new(move |agent, metrics| {
        let (engine, target, tools, tasks) =
            (engine.clone(), target.clone(), tools.clone(), tasks.clone());
        Box::pin(async move {
            engine
                .session(&target, &tools, task(&tasks, agent), &metrics)
                .await
        })
    })
}

#[cfg(test)]
pub(crate) mod tests;
