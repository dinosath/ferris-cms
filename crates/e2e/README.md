# e2e

End-to-end tests for ferriscms. The suite is **fully self-contained**: it does
**not** use Docker containers or testcontainers for the database or the
browser.

- **Database:** [Turso](https://github.com/tursodatabase/turso) — a local,
  SQLite-compatible database engine. Each test provisions a fresh Turso
  database file in a temp directory and runs the ferriscms server
  **in-process** against it (through the existing SQLite backend).
- **Browser:** [Obscura](https://github.com/h4ckf0r0day/obscura) — a lightweight
  Rust headless browser that is a drop-in replacement for headless Chrome. The
  harness launches `obscura serve` as a local subprocess and the tests drive it
  over CDP with [playwright-rs](https://crates.io/crates/playwright-rs).
- **Screenshots:** every UI test saves a PNG of the page it visits to
  `target/e2e-screenshots/` (override with `E2E_SCREENSHOT_DIR`).

No Postgres container, no Chrome container.

## Requirements

- **Rust toolchain** (a stable recent version).
- **`obscura` binary** on `PATH` (install from
  [Obscura releases](https://github.com/h4ckf0r0day/obscura/releases)). It is
  spawned per test with `--allow-private-network`, because the in-process
  server listens on `127.0.0.1` (Obscura blocks loopback by default).
- **Node.js 18+** for the playwright-rs driver. `playwright-rs` is a pure Rust
  API but drives Microsoft's Playwright *server*, which is a Node program. The
  crate bundles the driver; Node must be on `PATH`.
- **Built Dioxus WASM UI.** Point `FERRISCMS_UI_DIR` at a built WASM bundle
  (e.g. `target/dx/ferriscms/release/web/public`) before running the UI tests,
  or use a server build with the UI embedded. Build it with
  `cd crates/app && dx build --web --release` (requires `dx`, `wasm-bindgen`,
  `wasm-opt`, and `esbuild` on `PATH`).

`playwright-rs` needs OpenSSL for its native-TLS websocket; this repo builds it
**vendored** (see `crates/e2e/Cargo.toml`), so no system `libssl-dev` is
required.

## Running

The tests boot everything themselves; just run the suite:

```bash
cargo test -p e2e
```

Each `#[tokio::test]` boots its own stack and tears it down on completion.

## Configuration

| Env var               | Default                                  | Purpose                                              |
|-----------------------|------------------------------------------|------------------------------------------------------|
| `FERRISCMS_UI_DIR`    | *(embedded UI)*                           | Directory of a built Dioxus WASM UI for the UI tests |
| `E2E_SCREENSHOT_DIR`  | `target/e2e-screenshots`                  | Where each UI test saves its PNG screenshot          |
| `FERRISCMS_URL`       | `http://127.0.0.1:1337`                   | (Legacy) server URL when managing it externally      |
| `FERRISCMS_BROWSER_URL` | `http://127.0.0.1:9222`                 | (Legacy) CDP endpoint when managing the browser externally |

## Tests

- `tests/api_e2e.rs` — CRUD REST tests using `reqwest` against a fresh Turso
  database: register super admin → login (JWT) → create a content type → then
  Create, Read (single + list), Update and Delete content entries via the admin
  Content Manager API, plus a public (no-auth) read.
- `tests/ui_e2e.rs` — playwright-rs UI tests driving the Obscura headless
  browser: the embedded Dioxus WASM UI loads, hydrates, and navigates, and the
  Content-Type Builder screen no longer shows the "http error: builder error"
  bug (relative API URLs on the web target). Each test saves a screenshot.
- `tests/ui_screens.rs` — comprehensive playwright-rs screen tests covering
  every screen, the sidebar, and the main modals/inputs in the app's default
  (unauthenticated) state: Home + all four main screens (Content Manager,
  Content-Type Builder, Media Library, Settings), the CTB "create collection
  type" modal (inputs + toggles), the Settings sections and their create
  modals, and the Login/Register screens with their inputs. These work without
  authentication and reliably cover the full UI surface.
- `tests/ui_flows.rs` — playwright-rs UI *flow* tests (playwright-rs + Rust
  only). The admin account is provisioned over the HTTP API, then the tests log
  in, log out and back in, and create a collection type via the Content-Type
  Builder through the browser. Interaction is driven with `page.evaluate`
  (native input value setter + bubbling `input` event, and `.click()`), reading
  the DOM through bounded polls because the debug WASM hydration overlay is
  unstable. **Note:** these flow tests currently expose a Dioxus WASM app bug —
  submitting the login/register form crashes the app (the document collapses to
  a blank styled shell). The input values are set correctly and the same
  credentials log in over the HTTP API, so this is an app defect, not a test
  problem; the tests act as regression coverage for it until it is fixed.

Backend integration-test coverage for the API suite lives in
`crates/api-rest/tests` (`auth_workflow.rs`, `api_surface.rs`,
`coverage_deep.rs`, `coverage_edges.rs`).

[playwright-rs]: https://crates.io/crates/playwright-rs
