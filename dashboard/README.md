# Endemic dashboard

The Next.js console for Endemic: start managed runs and read run reports.

## Settings

Every setting is explicit. `npm run dev` and `npm run build` stop with an error when a required one is missing or invalid.

| Variable | Required | Meaning |
| --- | --- | --- |
| `ENDEMIC_MCP_URL` | yes | Your deployment's `/mcp` URL, shown in the Setup page snippets. `https:` only. Public. |
| `ENDEMIC_API_URL` | for live runs | Runner base URL. The Next.js server proxies `/api/*` to it. |
| `ENDEMIC_API_TOKEN` | with `ENDEMIC_API_URL` | Operator token. Added to proxied requests by the Next.js server; it never reaches the browser bundle. |

Without `ENDEMIC_API_URL`, live mode is off and the dashboard only opens saved reports. `npm start` serves the last build, so build with the variables you want it to show.

## Install and run

```sh
cd dashboard
npm install
export ENDEMIC_MCP_URL=https://endemic.example.com/mcp
ENDEMIC_API_URL=https://runner.example.com ENDEMIC_API_TOKEN=... npm run dev
```

**New run** asks for the target URL (plus an OpenAPI spec URL for an HTTP API), a staging bearer token, and an optional average-user use case. The token is sent once with `POST /runs` and never stored or shown again. While a run is running the dashboard polls `GET /runs/{id}` every 2 s.

## Open a saved report

Save a report with `curl ... /runs/{id} -o run.json` (see the Setup page) and use **Open report**. In dev, `http://localhost:3000/?fixture=sample` loads `fixtures/sample-run.json` instead.

Other dev-only query parameters, used for headless screenshots: `layout=<url-encoded layout JSON>` seeds the stored panel layout, `panels=open` opens the Panels menu, `sheet=form` or `sheet=review` opens New run with example values. Production builds ignore them.

## Panels

Drag a panel by its title bar, resize it from the lower-right corner, and hide it with its close button. **Panels** shows or hides each panel and resets the layout. The layout is saved in this browser under `endemic.dashboard.layout.v2`.

## Check and build

```sh
npm test
npm run lint
npm run build
```
