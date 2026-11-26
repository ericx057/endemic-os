mod api;
mod config;
mod egress;
mod http_api;
mod mcp;
mod open_weight;
mod plan;
mod request;
mod runner;
mod target;
mod task_mix;

use std::{env, sync::Arc};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env: config::Env = &|name| env::var(name);
    let engine = Arc::new(open_weight::Engine::from_env(env)?);
    let planner = Arc::new(plan::Planner::from_env(env, engine)?);
    let settings = api::Settings::from_env(env)?;
    let listen = config::required(env, "LISTEN_ADDR")?;
    let app = api::App::new(settings, move |request| {
        let planner = planner.clone();
        async move { planner.prepare(request).await }
    });
    let listener = TcpListener::bind(&listen)
        .await
        .map_err(|error| anyhow::anyhow!("LISTEN_ADDR {listen} cannot be bound: {error}"))?;
    axum::serve(listener, api::router(app)).await?;
    Ok(())
}
