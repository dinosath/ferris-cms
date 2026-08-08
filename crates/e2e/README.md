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
  harness launches `obscura serve` as a local subprocess, and the tests drive it
  over CDP with Playwright ([playwright-rs](https://crates.io/crates/playwright-rs))
  and Puppeteer.

No Postgres container, no Chrome container.

## Requirements

- A Rust toolchain (playwright-rs bundles its own Node driver at build time; no
  system Node is required).
- The `obscura` binary on `PATH` (install from
  [Obscura releases](https://github.com/h4ckf0r0day/obscura/releases) or `cargo
  install obscura`). It is spawned by the harness per test.
- For the UI tests, the Dioxus WASM admin UI must be reachable at the server
  root. Point `FERRISCMS_UI_DIR` at a built WASM bundle (e.g.
  `target/dx/ferriscms/release/web`) before running them, or use a server build
  with the UI embedded.

## Running

The tests boot everything themselves; just run the suite:

```bash
cargo test -p e2e
```

Each `#[tokio::test]` boots its own stack and tears it down on completion.

## Configuration

| Env var             | Default                                  | Purpose                                              |
|---------------------|------------------------------------------|------------------------------------------------------|
| `FERRISCMS_UI_DIR`  | *(embedded UI)*                           | Directory of a built Dioxus WASM UI for the UI tests |
| `FERRISCMS_URL`     | `http://127.0.0.1:1337`                   | (Legacy) server URL when managing it externally      |
| `FERRISCMS_BROWSER_URL` | `http://127.0.0.1:9222`               | (Legacy) CDP endpoint when managing the browser externally |

`FERRISCMS_URL` / `FERRISCMS_APP_URL` / `FERRISCMS_BROWSER_URL` are only used if
you prefer to run the server / browser externally instead of via
[`harness::E2eHarness`]; the harness is used by default.

## Tests

- `tests/api_e2e.rs` — CRUD REST tests using `reqwest` against a fresh Turso
  database: register super admin → login (JWT) → create a content type → then
  Create, Read (single + list), Update and Delete content entries via the admin
  Content Manager API, plus a public (no-auth) read.
- `tests/ui_e2e.rs` — Playwright UI tests driving the Obscura headless browser:
  the embedded Dioxus WASM UI loads, hydrates, and navigates, and the
  Content-Type Builder screen no longer shows the "http error: builder error"
  bug (relative API URLs on the web target).

[playwright-rs]: https://crates.io/crates/playwright-rs
