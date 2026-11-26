# Connect a coding agent to Endemic

Use your Endemic deployment’s HTTPS `/mcp` URL and its `API_BEARER_TOKEN`. Replace `https://endemic.example.com/mcp` in the examples with that URL. Keep the token in the client’s environment or credential settings. The token authorizes run operations, so share it only with operators who can start and cancel tests.

The client must be able to reach the deployment. A hosted agent may connect from a different network than your laptop.

## Cursor

Merge this entry into `~/.cursor/mcp.json` for your own use or `.cursor/mcp.json` for the project. Set `API_BEARER_TOKEN` in the environment that launches Cursor and restart Cursor after changing it. In **Customize → MCPs**, enable `endemic` and check that the three tools appear. [Cursor MCP configuration](https://cursor.com/docs/mcp)

```json
{
  "mcpServers": {
    "endemic": {
      "url": "https://endemic.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${env:API_BEARER_TOKEN}"
      }
    }
  }
}
```

## Claude Code

Merge this entry into the project root’s `.mcp.json`. Set `API_BEARER_TOKEN` in the environment that launches Claude Code. Start Claude Code from the project and use `/mcp` to approve the configuration and check the connection. [Claude Code MCP configuration](https://code.claude.com/docs/en/mcp#environment-variable-expansion-in-mcpjson)

```json
{
  "mcpServers": {
    "endemic": {
      "type": "http",
      "url": "https://endemic.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${API_BEARER_TOKEN}"
      }
    }
  }
}
```

Or add the same entry from the CLI. Single quotes keep your shell from expanding the token into the file; Claude Code expands `${API_BEARER_TOKEN}` when it connects. Check it with `claude mcp list`.

```sh
claude mcp add --transport http --scope project endemic https://endemic.example.com/mcp --header 'Authorization: Bearer ${API_BEARER_TOKEN}'
claude mcp list
```

## Codex

Set `API_BEARER_TOKEN` in the environment that launches Codex. Register Endemic with the CLI, then use `/mcp` in a new Codex session to verify the connection. The CLI and IDE extension share this configuration. [Codex MCP quickstart](https://developers.openai.com/learn/docs-mcp)

```sh
codex mcp add endemic --url https://endemic.example.com/mcp --bearer-token-env-var API_BEARER_TOKEN
codex mcp list
```

The same server in `~/.codex/config.toml` (or a trusted project’s `.codex/config.toml`). `bearer_token_env_var` names the variable; it does not hold the token. [Codex MCP configuration](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)

```toml
[mcp_servers.endemic]
url = "https://endemic.example.com/mcp"
bearer_token_env_var = "API_BEARER_TOKEN"
```

## Devin

1. Open **Customize → MCPs**, choose **+ → New MCP**, and name it `endemic`.
2. Select **HTTP** and enter `https://endemic.example.com/mcp` as the server URL.
3. Under **Authentication**, choose **Authorization header**. Set the header name to `Authorization` and enter `Bearer ` followed by the Endemic token as its value.
4. Select **Add**, then **Test tools**. Personal setup is available to individual members. For organization-wide setup, select the organization and use **Add custom MCP** with the **Manage MCP Servers** permission. [Devin custom MCP setup](https://docs.devin.ai/work-with-devin/mcp#setting-up-a-custom-mcp-server)

## GitHub Copilot (VS Code agent mode)

Put this entry in `.mcp.json` at the workspace root, or in `~/.copilot/mcp-config.json` (`$COPILOT_HOME/mcp-config.json` when set) for your own use. Copilot expands `${API_BEARER_TOKEN}` in header values from the environment of the host running the session; write it with braces, since a bare `$NAME` stays literal. Run **MCP: List Servers** from the Command Palette, or **Chat: Open Customizations → MCP Servers**, to check that `endemic` is running, then ask Copilot to list Endemic’s tools and expand the tool call to confirm it used the server. [VS Code MCP configuration](https://code.visualstudio.com/docs/agents/reference/mcp-configuration)

```json
{
  "mcpServers": {
    "endemic": {
      "type": "http",
      "url": "https://endemic.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${API_BEARER_TOKEN}"
      },
      "tools": ["*"]
    }
  }
}
```

`code --add-mcp` adds a server to your VS Code user profile instead, in the older editor-managed format, which writes environment references as `${env:NAME}`:

```sh
code --add-mcp '{"name":"endemic","type":"http","url":"https://endemic.example.com/mcp","headers":{"Authorization":"Bearer ${env:API_BEARER_TOKEN}"}}'
```

## Windsurf

Windsurf’s Cascade MCP documentation now points to Devin Desktop. Add Endemic in **Customize → MCPs** with the steps in the Devin section, or merge this entry into `.devin/mcp_config.local.json` in the project (kept out of git) or `~/.config/devin/mcp_config.json` for your own use. The documentation does not say whether `${env:...}` expands inside `headers`; if the server shows **Needs authentication**, set the header in **Customize → MCPs** instead. In **Customize → MCPs**, refresh the list and check that `endemic` is on, or run `devin mcp list`. [Devin Desktop MCP](https://docs.devin.ai/desktop/cascade/mcp), [MCP configuration file](https://docs.devin.ai/cli/extensibility/mcp/configuration)

```json
{
  "mcpServers": {
    "endemic": {
      "url": "https://endemic.example.com/mcp",
      "transport": "http",
      "headers": {
        "Authorization": "Bearer ${env:API_BEARER_TOKEN}"
      }
    }
  }
}
```

## Use the tools

| Tool | Arguments |
| --- | --- |
| `start_run` | `target_url`, `bearer`, optional `openapi_url` and `use_case` |
| `get_run` | `id` from a start response |
| `cancel_run` | `id` of an active run |

To verify discovery without generating load, ask your agent to list Endemic’s tools. When you’re ready to generate staging traffic, try:

> Use Endemic to start a run against https://staging.example.com/mcp with my staging token, for shoppers who search the catalog and check out. Return the run ID, the plan it chose, and its report once it finishes.

`start_run` takes the same fields as `POST /runs`: `target_url` is an MCP endpoint, or with `openapi_url` the base URL of an HTTP API. Endemic decides the tools, tasks, parallelism, and agent ramp, and the report shows them under `plan`. The `bearer` argument passes through your MCP client and its model, so give it a staging-only token; Endemic holds it in memory for the run and never returns it.

## Drive Endemic from the command line

The REST API sits beside `/mcp` on the same deployment and takes the same token. Keep the token in `API_BEARER_TOKEN`; the commands read it from the environment so it never appears in your shell history. `${ENDEMIC%/mcp}` turns the MCP URL into the API base.

```sh
ENDEMIC=https://endemic.example.com/mcp
```

Start a run. The response is the new run’s report; note its `id`. Keep the staging token in `STAGING_TOKEN`; add `"openapi_url"` inside `target` for an HTTP API, and `"use_case"` to steer the generated tasks.

```sh
curl -sS -X POST "${ENDEMIC%/mcp}/runs" \
  -H "Authorization: Bearer $API_BEARER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"target":{"url":"https://staging.example.com/mcp"},"credentials":{"bearer":"'"$STAGING_TOKEN"'"}}'
```

Get a run’s report, or cancel an active run:

```sh
curl -sS "${ENDEMIC%/mcp}/runs/1" -H "Authorization: Bearer $API_BEARER_TOKEN"
curl -sS -X DELETE "${ENDEMIC%/mcp}/runs/1" -H "Authorization: Bearer $API_BEARER_TOKEN"
```

Save a report to a file, then open it in the dashboard with **Open report**:

```sh
curl -sS "${ENDEMIC%/mcp}/runs/1" -H "Authorization: Bearer $API_BEARER_TOKEN" -o run-1.json
```

The runner keeps only the latest run in memory, so save reports you want to keep. It answers `409` while another run is active and `422` with an `error` message when the body is invalid. A run whose discovery or task generation fails reports `status: failed` with the reason in `error`.

## Network

The client must reach the deployment over HTTPS. Hosted agents often connect from a different network than your laptop. Endemic refuses `/mcp` requests that carry an `Origin` header, so browser pages cannot call it; desktop clients and CLIs do not send one.
