//! Startup settings: every value is an explicit environment variable, checked once, with no
//! defaults. A missing or malformed variable stops startup with a message naming it.

use anyhow::{Context, Result, bail};
use std::{env::VarError, str::FromStr, time::Duration};

/// Variable lookup; `main` passes `std::env::var`, tests pass a fixed map.
pub type Env<'a> = &'a dyn Fn(&str) -> Result<String, VarError>;

pub fn required(env: Env, name: &str) -> Result<String> {
    optional(env, name)?.with_context(|| format!("{name} is required"))
}

/// For settings whose absence turns a feature off.
pub fn optional(env: Env, name: &str) -> Result<Option<String>> {
    match env(name) {
        Ok(value) => Ok(Some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => bail!("{name} must be UTF-8"),
    }
}

pub fn positive<T: FromStr + PartialOrd + Default>(env: Env, name: &str) -> Result<T> {
    required(env, name)?
        .parse()
        .ok()
        .filter(|value| *value > T::default())
        .with_context(|| format!("{name} must be a positive integer"))
}

pub fn seconds(env: Env, name: &str) -> Result<Duration> {
    positive(env, name).map(Duration::from_secs)
}

/// Client for an operator-configured endpoint: no proxy, redirects, or retries, so every
/// failure is reported instead of hidden.
pub fn http_client(connect: Duration, timeout: Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(connect)
        .timeout(timeout)
        .build()?)
}

#[cfg(test)]
pub(crate) mod tests;
