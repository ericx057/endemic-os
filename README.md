# Endemic

> Endemic is owned and maintained by Ark Security Company (ASC). Endemic is open sourced under the [Apache License 2.0](LICENSE) for use. Endemic is also offered by ASC as a managed service with SSO and MFA, role-based access control, signed audit logs, an encrypted credential vault, and usage policies with spend caps.

Endemic load tests your staging software with traffic from real AI agents. Give the agents a task and a set of approved tools, and they decide which calls to make while running concurrently. Endemic shows when tool calls arrive in bursts, how latency and errors change as you add agents, and whether an error came from your software or from model throttling.

A target is an MCP server (Streamable HTTP) or an HTTP API described by an OpenAPI document. Agents run on any vLLM or OpenAI-compatible chat completions endpoint that supports tool calling.

## Why

A fixed request script checks throughput, but agents don't follow scripts. They pick their own tool sequences, retry or change course when they hit errors, call APIs directly instead of through a UI, and run many workflows at once. That produces bursts and repeated calls that human traffic never does. Endemic tests against that load with agents doing the actual work.

## Quick setup

You need Rust (stable, edition 2024), Node 20 or later for the dashboard, and an inference endpoint. For example, serve a tool-calling model with vLLM behind HTTPS:

```sh
vllm serve <model> --enable-auto-tool-choice --tool-call-parser <parser> --api-key "$VLLM_API_KEY"
```

1. Copy the example settings and fill them in. Every setting is required and has no default. Startup stops and names any setting that is missing or malformed.

   ```sh
   cp endemic.env.example endemic.env
   ```

2. Start the runner:

   ```sh
   set -a; . ./endemic.env; set +a
   cargo run --release
   ```

3. Start a run against your staging target:

   ```sh
   curl -X POST http://127.0.0.1:8080/runs -H "Authorization: Bearer $API_BEARER_TOKEN" \
     -H 'Content-Type: application/json' \
     -d '{"target":{"url":"https://staging.example.com/mcp"},"credentials":{"bearer":"'"$STAGING_TOKEN"'"},"use_case":"Shoppers search the catalog and check out."}'
   ```

4. Read the report with `GET /runs/{id}`, or open it in the dashboard (see [dashboard/README.md](dashboard/README.md)).

See [docs/reference.md](docs/reference.md) for every setting, the run API, and the report fields.

## Contributing

Contributions are welcome under the Apache License 2.0. Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

- Every change goes through a pull request to `main`. Direct pushes and force pushes are blocked.
- A pull request needs one approving review from a code owner and a passing CI run before it merges.
- CI runs `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and the dashboard's type check, lint, tests and build.
- Report security issues privately through GitHub's private vulnerability reporting on this repository, not in a public issue.
