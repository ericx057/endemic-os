//! The managed-run request (docs/dashboard-plan.md, "Managed runs"): what the user supplies,
//! validated before anything is fetched.

use crate::runner::Pacing;
use reqwest::{Url, header::HeaderValue};
use serde::Deserialize;
use serde_json::error::Category;
use std::fmt;

/// Sanity bounds on user input (the use case bound is the documented contract).
pub const USE_CASE_LIMIT: usize = 4096;
const BEARER_LIMIT: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    target: TargetBody,
    credentials: Credentials,
    use_case: Option<String>,
    hold_agents: Option<usize>,
    #[serde(default)]
    pacing: Pacing,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetBody {
    url: String,
    openapi_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    bearer: Bearer,
}

/// The staging token. It lives only as long as the run that uses it, and Debug never shows it.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Bearer(String);

impl fmt::Debug for Bearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Bearer(redacted)")
    }
}

impl Bearer {
    /// The Authorization header, marked sensitive so the HTTP stack never logs it.
    pub fn header(&self) -> HeaderValue {
        let mut value = HeaderValue::from_str(&format!("Bearer {}", self.0))
            .expect("validated visible ASCII is a valid header value");
        value.set_sensitive(true);
        value
    }
}

#[derive(Debug)]
pub struct RunRequest {
    pub url: Url,
    /// Present for an HTTP API target; absent means `url` is an MCP endpoint.
    pub openapi_url: Option<Url>,
    pub bearer: Bearer,
    pub use_case: Option<String>,
    pub hold_agents: Option<usize>,
    pub pacing: Pacing,
}

impl RunRequest {
    pub fn parse(body: &[u8]) -> Result<Self, String> {
        let body: Body = serde_json::from_slice(body).map_err(describe)?;
        Self::new(
            &body.target.url,
            body.target.openapi_url.as_deref(),
            body.credentials.bearer,
            body.use_case,
            body.hold_agents,
            body.pacing,
        )
    }

    pub fn new(
        url: &str,
        openapi_url: Option<&str>,
        bearer: Bearer,
        use_case: Option<String>,
        hold_agents: Option<usize>,
        pacing: Pacing,
    ) -> Result<Self, String> {
        if !(1..=BEARER_LIMIT).contains(&bearer.0.len())
            || !bearer.0.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(format!(
                "credentials.bearer must be 1 to {BEARER_LIMIT} visible ASCII bytes"
            ));
        }
        if use_case
            .as_ref()
            .is_some_and(|text| text.trim().is_empty() || text.len() > USE_CASE_LIMIT)
        {
            return Err(format!(
                "use_case must be 1 to {USE_CASE_LIMIT} bytes and not blank"
            ));
        }
        if hold_agents == Some(0) {
            return Err("hold_agents must be at least 1".into());
        }
        Ok(Self {
            url: https_url(url, "target.url")?,
            openapi_url: openapi_url
                .map(|url| https_url(url, "target.openapi_url"))
                .transpose()?,
            bearer,
            use_case,
            hold_agents,
            pacing,
        })
    }
}

fn https_url(value: &str, field: &str) -> Result<Url, String> {
    Url::parse(value)
        .ok()
        .filter(|url| {
            url.scheme() == "https"
                && url.has_host()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
        })
        .ok_or_else(|| {
            format!("{field} must be an HTTPS URL without embedded credentials or a fragment")
        })
}

/// serde's type errors quote the offending value, which may be the token, so only messages
/// that name keys or positions are passed through.
fn describe(error: serde_json::Error) -> String {
    let text = error.to_string();
    let names_keys = text.starts_with("unknown field") || text.starts_with("missing field");
    if names_keys || error.classify() != Category::Data {
        return format!("invalid run request: {text}");
    }
    format!(
        "invalid run request at line {} column {}: expected {{\"target\":{{\"url\",\"openapi_url\"?}},\"credentials\":{{\"bearer\"}},\"use_case\"?,\"hold_agents\"?,\"pacing\"?}} with string values, an integer hold_agents, and a pacing object",
        error.line(),
        error.column()
    )
}

#[cfg(test)]
pub(crate) mod tests;
