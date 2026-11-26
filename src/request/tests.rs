use super::*;

const TOKEN: &str = "staging-secret-token";

fn parse(body: &str) -> Result<RunRequest, String> {
    RunRequest::parse(body.as_bytes())
}

#[test]
fn accepts_the_managed_run_body() {
    let request = parse(
        r#"{"target":{"url":"https://staging.example.com/api","openapi_url":"https://staging.example.com/openapi.json"},
            "credentials":{"bearer":"staging-secret-token"},"use_case":"Shoppers check out.","hold_agents":200}"#,
    )
    .unwrap();
    assert_eq!(request.url.as_str(), "https://staging.example.com/api");
    assert_eq!(
        request.openapi_url.unwrap().as_str(),
        "https://staging.example.com/openapi.json"
    );
    assert_eq!(request.use_case.as_deref(), Some("Shoppers check out."));
    assert_eq!(request.hold_agents, Some(200));
    let header = request.bearer.header();
    assert_eq!(header, format!("Bearer {TOKEN}").as_str());
    assert!(header.is_sensitive());

    let minimal =
        parse(r#"{"target":{"url":"https://s.example.com/mcp"},"credentials":{"bearer":"t"}}"#);
    let minimal = minimal.unwrap();
    assert!(
        minimal.openapi_url.is_none()
            && minimal.use_case.is_none()
            && minimal.hold_agents.is_none()
    );
}

#[test]
fn rejects_invalid_requests_with_a_clear_message() {
    let body = |target: &str, bearer: &str, extra: &str| {
        format!(r#"{{"target":{target},"credentials":{{"bearer":{bearer}}}{extra}}}"#)
    };
    let ok_target = r#"{"url":"https://s.example.com"}"#;
    let ok_bearer = r#""t""#;
    let cases = [
        (
            body(r#"{"url":"http://s.example.com"}"#, ok_bearer, ""),
            "target.url must be an HTTPS URL",
        ),
        (
            body(r#"{"url":"https://u:p@s.example.com"}"#, ok_bearer, ""),
            "target.url must be an HTTPS URL",
        ),
        (
            body(r#"{"url":"https://s.example.com#x"}"#, ok_bearer, ""),
            "target.url must be an HTTPS URL",
        ),
        (
            body(
                r#"{"url":"https://s.example.com","openapi_url":"ftp://x"}"#,
                ok_bearer,
                "",
            ),
            "target.openapi_url must be an HTTPS URL",
        ),
        (
            body(ok_target, r#""""#, ""),
            "credentials.bearer must be 1 to 4096",
        ),
        (
            body(ok_target, r#""has space""#, ""),
            "credentials.bearer must be 1 to 4096",
        ),
        (
            body(ok_target, &format!(r#""{}""#, "x".repeat(4097)), ""),
            "credentials.bearer must be 1 to 4096",
        ),
        (
            body(ok_target, ok_bearer, r#","use_case":"""#),
            "use_case must be 1 to 4096 bytes",
        ),
        (
            body(ok_target, ok_bearer, r#","use_case":"   ""#),
            "use_case must be 1 to 4096 bytes",
        ),
        (
            body(
                ok_target,
                ok_bearer,
                &format!(r#","use_case":"{}""#, "x".repeat(4097)),
            ),
            "use_case must be 1 to 4096 bytes",
        ),
        (
            body(ok_target, ok_bearer, r#","hold_agents":0"#),
            "hold_agents must be at least 1",
        ),
        (
            body(ok_target, ok_bearer, r#","mode":"capacity""#),
            "invalid run request: unknown field `mode`",
        ),
        (
            body(r#"{"url":"https://s.example.com","x":1}"#, ok_bearer, ""),
            "invalid run request: unknown field `x`",
        ),
        (
            r#"{"target":{"url":"https://s.example.com"}}"#.to_owned(),
            "invalid run request: missing field `credentials`",
        ),
        ("not json".to_owned(), "invalid run request: expected"),
    ];
    for (body, expected) in cases {
        let error = parse(&body).err().unwrap_or_default();
        assert!(error.starts_with(expected), "{body}: {error}");
    }
}

#[test]
fn errors_and_debug_never_show_the_bearer() {
    // A token sent where an object belongs: serde's own message would quote it.
    let error = parse(&format!(
        r#"{{"target":{{"url":"https://s.example.com"}},"credentials":"{TOKEN}"}}"#
    ))
    .err()
    .unwrap();
    assert!(!error.contains(TOKEN), "{error}");
    assert!(error.starts_with("invalid run request"), "{error}");

    let bearer: Bearer = serde_json::from_value(serde_json::json!(TOKEN)).unwrap();
    assert_eq!(format!("{bearer:?}"), "Bearer(redacted)");
    let request = parse(&format!(
        r#"{{"target":{{"url":"https://s.example.com"}},"credentials":{{"bearer":"{TOKEN}"}}}}"#
    ))
    .unwrap();
    assert!(!format!("{request:?}").contains(TOKEN));
    assert!(!format!("{:?}", request.bearer.header()).contains(TOKEN));
}

impl RunRequest {
    /// A request for a plain-HTTP local fixture, which production validation refuses.
    pub(crate) fn loopback(url: &str, openapi_url: Option<&str>, use_case: Option<&str>) -> Self {
        Self {
            url: Url::parse(url).unwrap(),
            openapi_url: openapi_url.map(|url| Url::parse(url).unwrap()),
            bearer: Bearer(TOKEN.into()),
            use_case: use_case.map(str::to_owned),
            hold_agents: None,
            pacing: Default::default(),
        }
    }
}

pub(crate) const FIXTURE_TOKEN: &str = TOKEN;
