# e2e

End-to-end tests for ferriscms using [playwright-rs] to drive a real headless
Chrome against the containerized server, plus an HTTP-level backend test.

## Requirements

- Docker (the `ferriscms-server:local` image must exist — build it with the
  root `Dockerfile`).
- `chromedp/headless-shell` image (pulled automatically by compose).
- A Rust toolchain for building the test crate (playwright-rs bundles its own
  Node driver at build time; no system Node is required).

## Running

The tests do **not** start containers themselves. Start the stack with the
bundled compose file, then run the tests:

```bash
# 1. Build the server image (single self-contained binary, embedded UI)
docker compose -f docker-compose.e2e.yml up -d --build

# 2. Run the e2e suite
cargo test -p e2e

# 3. Tear down
docker compose -f docker-compose.e2e.yml down
```

The server image always embeds a **release** build of the Dioxus WASM UI
(optimized, no debug symbols, no devtools overlay). The build installs the
`wasm-opt`/`wasm-bindgen`/`esbuild` tooling dx needs, so it requires npm and
GitHub access at build time.

## Configuration

| Env var                 | Default                 | Purpose                                  |
|-------------------------|-------------------------|------------------------------------------|
| `FERRISCMS_URL`         | `http://127.0.0.1:1337` | ferriscms server URL (reachable from the test process) |
| `FERRISCMS_BROWSER_URL` | `http://127.0.0.1:9222` | Chrome CDP endpoint (reachable from the test process) |

## Tests

- `tests/api_e2e.rs` — backend REST workflow: first-run init → register super
  admin → login (JWT) → create a content type → list → create an entry → read
  it back via the public API.
- `tests/ui_e2e.rs` — Playwright UI tests: the embedded Dioxus WASM UI loads,
  hydrates, and navigates, and the Content-Type Builder screen no longer shows
  the "http error: builder error" bug (relative API URLs on the web target).

[playwright-rs]: https://crates.io/crates/playwright-rs
