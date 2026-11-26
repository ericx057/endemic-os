# Endemic reference

One Rust binary holds the HTTPS API, the planner, the scheduler, the metrics, the target clients, and the agent loop. Every request to the target carries a W3C `traceparent` header with one trace ID per agent session, so a burst in the report can be found in your own tracing backend.

## What you see in a run

`GET /runs/{id}` returns `status`, `highest_passing_agents`, `plan`, `error`, and `stages[]`.

`plan` records what Endemic decided: `target_kind` (`mcp` or `http`), `tools`, `parallel_tools` (read-only tools dispatched concurrently), the generated `tasks`, `hold_agents` (the requested hold, or null for a ramp), `pacing`, `faults`, `task_mix` (present only for a mixed run: each persona's `name`, `weight`, `max_model_turns`, `max_tool_calls`, and its slice of `tasks`), and `agents_per_stage`, which grows as stages start. `status` is `running`, `cancelled`, `failed` (the run stopped before any stage: discovery, the address guard, or task generation; `error` says why, and `plan` keeps what was decided), `failed_stage`, `reached_ceiling`, or `completed` (a held stage passed). Each stage reports:

| Question | Report fields |
| --- | --- |
| When do tool calls arrive in bursts? | `traffic.calls_per_window` (tool calls per `traffic.window_ms` window), `traffic.peak_calls_per_window`, `traffic.p95_calls_per_occupied_window` |
| How do latency and errors change as agents are added? | `target`, `inference`, and `runner` latencies (`samples`, `p95_ms`, `p99_ms`) per stage, with `agents`, `started`, `completed`, and `passed` |
| Is it my software or the model? | `target_errors`, `inference_errors`, `inference_throttles` (HTTP 429 from the inference server), `runner_errors`, `interrupted` |
| What do agents call most? | `hotspots`: tool, calls, share, target errors, p95/p99 latency. HTTP rows read as `operationId (METHOD /path/{template})` |
| What did one session do? | `traffic.session_samples`: up to `TRAFFIC_SESSION_SAMPLES` sessions, each with its `trace_id` and `dispatches` (relative time and tool names; no arguments or results) |
| What did inference use? | `input_tokens`, `output_tokens`, `tokens` |
| Did the target and agents degrade gracefully under faults? | `faults` (the request's faults active this stage), `injected` (per tool: calls the runner `delayed`, failed with `errors`, or failed as `timeouts`; none reached the target), `injected_failures` (sessions an injected fault ended, counted apart from `target_errors` and outside the error budget) |
| Why did it fail? | `failed_criterion` (the first pass rule a failing stage broke: `held`, `all_agents_active`, `sessions_completed`, `runner_p95`, `runner_p99`, or `error_budget`) |

Inference throttles are reported apart from target errors, so a model limit does not become a capacity claim about your software.

A `failed_stage` run also carries `explanation`: the failing stage's `agents` and `criterion`, `baseline_agents` (the last passing stage, or null), `error_rate` and `p95_ms` (the tool whose value rose most since the baseline, `from` and `to`, and `began_at_agents`, where the rise started; null when nothing rose), and `sample_trace_ids` (sampled sessions in the failing stage that called those tools). Every other run has `explanation: null`.

## Settings

Every variable is required and has no default; a missing or malformed one stops startup with a message naming it. Keep secrets in the environment or a secret manager, never in source control. Targets and their credentials are not configured here: they arrive with each run. [`endemic.env.example`](../endemic.env.example) lists them all with example values.

### API and ramp

| Variable | Value |
| --- | --- |
| `LISTEN_ADDR` | Bind address |
| `API_BEARER_TOKEN` | Operator token, at least 32 visible ASCII characters |
| `API_BODY_LIMIT_BYTES` | Largest request body the API accepts |
| `RUN_START_AGENTS` | Agents in the first stage, at most `RUN_MAX_AGENTS` |
| `RUN_MAX_AGENTS` | Agent ceiling; stages double from `RUN_START_AGENTS` up to it |
| `RUN_HOLD_SECONDS` | How long each stage holds its agents |
| `RUN_DRAIN_SECONDS` | Grace for in-flight sessions after a hold before they count as `interrupted` |
| `STAGE_RUNNER_P95_MS`, `STAGE_RUNNER_P99_MS` | Runner latency a passing stage stays within |
| `STAGE_ERROR_BUDGET_PERCENT` | 1 to 99: errors plus interruptions a passing stage stays under, per completed session |
| `TRAFFIC_WINDOW_MS` | Width of the burst windows in `traffic.calls_per_window` |
| `TRAFFIC_SESSION_SAMPLES` | Sessions per stage whose dispatch timeline is kept |

### Planning and targets

| Variable | Value |
| --- | --- |
| `PLAN_TASK_COUNT` | Distinct tasks generated per run; agents take them round-robin |
| `RUN_MAX_TOOLS` | Most tools a target may expose; more fails the run |
| `OPENAPI_SPEC_LIMIT_BYTES` | Largest OpenAPI document fetched for an HTTP target |
| `TARGET_CONNECT_TIMEOUT_SECONDS`, `TARGET_TIMEOUT_SECONDS` | Connect and whole-request timeouts for target requests |
| `TARGET_REQUEST_LIMIT_BYTES`, `TARGET_RESPONSE_LIMIT_BYTES` | Largest tool call sent and response received |

### Inference and agents

| Variable | Value |
| --- | --- |
| `VLLM_URL` | HTTPS `.../v1/chat/completions` URL of a vLLM or OpenAI-compatible server with tool calling, without credentials, query, or fragment |
| `VLLM_API_KEY` | Inference bearer credential, 1 to 4096 visible ASCII characters |
| `VLLM_MODEL` | Served model name |
| `VLLM_MAX_TOKENS` | Output token cap per model turn, including the planning call |
| `VLLM_TEMPERATURE` | 0.0 to 2.0 |
| `VLLM_CHAT_TEMPLATE_KWARGS` | JSON object passed to the chat template, such as the model's reasoning switch |
| `VLLM_CONNECT_TIMEOUT_SECONDS`, `VLLM_TIMEOUT_SECONDS` | Connect and whole-request timeouts for inference |
| `VLLM_REQUEST_LIMIT_BYTES`, `VLLM_RESPONSE_LIMIT_BYTES` | Largest completion request sent and response received |
| `VLLM_TOOL_ARGUMENTS_LIMIT_BYTES` | Largest tool-call arguments accepted from the model |
| `AGENT_MAX_MODEL_TURNS`, `AGENT_MAX_TOOL_CALLS` | Per-session budgets |
| `AGENT_RESULT_LIMIT_BYTES` | Largest tool result passed back to the model; a larger one counts as a target error |

## Start a run

```sh
curl -X POST "$ENDEMIC/runs" -H "Authorization: Bearer $API_BEARER_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"target":{"url":"https://staging.example.com/mcp"},"credentials":{"bearer":"'"$STAGING_TOKEN"'"},"use_case":"Shoppers search the catalog and check out."}'
```

The body is exactly `{"target":{"url","openapi_url"?},"credentials":{"bearer"},"use_case"?,"hold_agents"?,"pacing"?,"faults"?,"task_mix"?}`; unknown fields are rejected.

- `target.url` (HTTPS): an MCP endpoint, or with `target.openapi_url` (HTTPS) the base URL of an HTTP API. The MCP endpoint must support the `2025-11-25` Streamable HTTP lifecycle. The OpenAPI document must be OpenAPI 3 JSON; it is fetched with the bearer only when it shares the target's origin.
- `credentials.bearer`: 1 to 4096 visible ASCII characters. It is held in memory for the run only, sent as `Authorization: Bearer` to the target, and never returned, logged, or written to a report.
- `use_case` (optional, 1 to 4096 bytes): steers the generated tasks.
- `hold_agents` (optional, 1 to `RUN_MAX_AGENTS`): run one stage at exactly this many agents to check the target holds them. Omit it to find the breaking point.
- `pacing` (optional): `start_spread_ms` (each agent starts at a random offset below it), `think_min_ms` and `think_max_ms` (random pause between an agent's sessions), and `sync_sessions` (agents begin every session together, in waves). Each field defaults to 0 or false, which starts all agents together with no pauses. Spread and pauses must stay below `RUN_HOLD_SECONDS`.
- `faults` (optional): a list of `{"tool","first_stage","last_stage","percent","effect"}`. During stages `first_stage` to `last_stage` (1-based, inclusive), `percent` (1 to 100) of the runner's calls to `tool` get `effect`: `{"kind":"latency","delay_ms"}` (a wait before the call, below `TARGET_TIMEOUT_SECONDS`), `{"kind":"error"}`, or `{"kind":"timeout"}` (a `TARGET_TIMEOUT_SECONDS` wait, then a failure). Failed calls are never sent to the target; a batch with one is not sent at all. Which calls are hit is drawn deterministically from the run id, stage, agent, session round, and call number. `tool` must be one the target exposes, checked once discovery finishes. A session that hits an injected failure ends, as on any tool failure.
- `task_mix` (optional, replaces `use_case`): a list of personas `{"name","weight","use_case"?,"max_model_turns"?,"max_tool_calls"?}`. Names are 1 to 64 bytes and unique, weights are at least 1 and sum to 100. Each persona gets its own `PLAN_TASK_COUNT` generated tasks, steered by its `use_case`, and every 100 consecutive agents split exactly by weight. `max_model_turns` and `max_tool_calls` default to, and may not exceed, `AGENT_MAX_MODEL_TURNS` and `AGENT_MAX_TOOL_CALLS`.

At run start Endemic discovers every tool (MCP `tools/list`, or every OpenAPI operation by `operationId`), makes one inference call that turns the tools and use case into `PLAN_TASK_COUNT` tasks, then runs one stage at `hold_agents`, or without it ramps agents from `RUN_START_AGENTS`, doubling after every passing stage up to `RUN_MAX_AGENTS` and stopping at the first failing stage. A passing ceiling is a lower bound on capacity. Read-only tools (HTTP GET and HEAD, MCP `readOnlyHint: true`) are dispatched in parallel; everything else runs in model order. One run is active per process.

Every target and spec host is resolved once and refused unless all its addresses are public unicast (no loopback, private, link-local or metadata, CGNAT, multicast, reserved, or documentation ranges). The run's connections are pinned to the vetted addresses, other names do not resolve, and redirects are not followed.

HTTP operations become tools with `path`, `query`, and `body` inputs (path and query parameters, `application/json` body). Each call returns `{"status": <code>, "body": <JSON or text>}`. Non-2xx responses count as target errors, and header and cookie parameters are not sent. Operations without an `operationId`, duplicate ids, non-local `$ref`s, and cycles fail the run.

## Run API

Every request needs `Authorization: Bearer <API_BEARER_TOKEN>`, except `/health`.

| Request | Behavior |
| --- | --- |
| `POST /runs` | Starts a run and returns its report (with `id`) as `202`; `409` if a run is active, `422` with `{"error": "..."}` if the body is invalid |
| `GET /runs/{id}` | Returns the latest report, or `404` |
| `DELETE /runs/{id}` | Cancels active work, including planning, and returns `202`; `409` if the run already finished |
| `GET /health` | Returns `204` for the load balancer health check |
| `POST /mcp` | The same operations as MCP tools (`start_run`, `get_run`, `cancel_run`); requests with an `Origin` header are refused |

Only the latest run is kept, in memory. To drive Endemic from Cursor, Claude Code, Codex, or Devin, see [MCP client setup](mcp-clients.md).

## Dashboard and reports

The dashboard plots scaling, error sources, hotspots, bursts, and session traces from a run report. It proxies to the runner and adds the token server-side; without a runner, it opens a saved `GET /runs/{id}` JSON. See [dashboard/README.md](../dashboard/README.md).

## How agents run

For each stage, the runner creates one Tokio task per agent and releases them together. Tasks share one inference client and the run's pinned HTTP connection pool to the target. Agent `i` pursues task `i mod PLAN_TASK_COUNT`. Each task opens a target session (for MCP: initialize and list the run's tools), gives its task to the model, executes the model's tool calls, and returns each result to the model in full. A session ends when the model stops after at least one tool call, or fails when it exceeds `AGENT_MAX_MODEL_TURNS` or `AGENT_MAX_TOOL_CALLS`. The task then starts a fresh session with new conversation state.

At a normal stage boundary the runner allows up to `RUN_DRAIN_SECONDS` for in-flight sessions to finish; the rest count as `interrupted`. Cancelling interrupts in-flight work immediately. A stage passes when it ran for the full hold, all requested agents were active at once, at least one session completed, runner p95 and p99 are within `STAGE_RUNNER_P95_MS` and `STAGE_RUNNER_P99_MS`, and errors plus interruptions stay under `STAGE_ERROR_BUDGET_PERCENT` of completed sessions.

Runner latency is time spent inside the session's own Rust polls, summed per completed session; target and inference latencies are wall-clock waits. For MCP, target timings cover initialization plus discovery as one operation, each tool call, and session cleanup.

## Verification

```sh
cargo test --locked
(cd dashboard && npm test)
```

Benchmark the runner against the local fixture with `BENCH_AGENTS` concurrent sessions:

```sh
BENCH_AGENTS=<agents> cargo test --release benchmark_runner -- --ignored --nocapture
```
