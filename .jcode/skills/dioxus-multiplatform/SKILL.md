---
name: dioxus-multiplatform
description: Scaffold and build multiplatform Dioxus apps (web WASM + native desktop) with an Axum backend in Rust. Use the dx CLI to scaffold, keep axum on the server side, and wire the client to the API via reqwest. Use this skill whenever building, migrating, or extending a Dioxus frontend backed by an Axum server.
allowed-tools: bash, read, write, edit, apply_patch, agentgrep, ls
---

# Dioxus Multiplatform + Axum Backend

Build a cross-platform Rust UI with [Dioxus](https://dioxuslabs.com) (one codebase
compiled to web via WASM and to native desktop), served by an **Axum** backend.

## When to use

- Creating a new Dioxus app from scratch.
- Migrating an existing UI (e.g. Makepad / web-only / framework-agnostic specs) to Dioxus.
- Adding new screens, routes, or widgets to an existing Dioxus app.

## 1. Tooling

- Install the Dioxus CLI: `cargo install dioxus-cli` (provides `dx`).
- Check the version: `dx --version`. The Dioxus 0.7 ecosystem is assumed below.
- Verify wasm target is installed: `rustup target add wasm32-unknown-unknown`.

## 2. Scaffolding (use the CLI)

Follow https://dioxuslabs.com/learn/0.7/tutorial/new_app — use the provided tooling,
don't hand-roll the skeleton.

```bash
dx new <app-name>
```

Select options for a multiplatform, backend-free SPA that talks to your Axum server:

- Template: **Bare-bones** for a simple app, **Jumpstart** for structure, or **Workspace**
  when you want separate crates per platform.
- Fullstack website: **false** (the backend is already Axum; don't mix Dioxus-Server).
- Router: **true** if you need client routing (recommended for multi-screen apps).
- TailwindCSS: optional (this skill uses inline styles/design tokens; set false unless you
  want Tailwind).
- LLM prompts: **false**.
- Default platform: **Web**.

The CLI generates a `Cargo.toml`, `Dioxus.toml`, an `assets/` folder, and `src/main.rs`.

### Feature wiring (multiplatform)

Dioxus apps are plain Cargo projects. Enable platform features in `Cargo.toml`:

```toml
[dependencies]
dioxus = { version = "0.7" }

[features]
default = ["web"]
web = ["dioxus/web"]
desktop = ["dioxus/desktop"]
mobile = ["dioxus/mobile"]
```

`dx serve` runs the app for web; `dx serve --desktop` (or `cargo run --no-default-features
--features desktop`) runs it natively.

## 3. Run / build

```bash
dx serve              # dev web server (defaults to http://127.0.0.1:8080)
dx serve --desktop    # native desktop window
dx build --web        # optimized wasm bundle
dx build --desktop    # optimized native binary
# or plain cargo, target-specific:
cargo build --target wasm32-unknown-unknown --no-default-features --features web
cargo build --no-default-features --features desktop
```

## 4. App skeleton

`src/main.rs` root:

```rust
use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! { "Hello from Dioxus" }
}
```

## 5. Connecting to an Axum backend

The Axum server exposes JSON REST routes. In the Dioxus client use `reqwest` with the
`json` and (for WASM) the `wasm` feature. Prefer a small typed client module.

```toml
[dependencies]
reqwest = { version = "0.12", features = ["json", "wasm"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

```rust
#[derive(serde::Deserialize)]
struct InitInfo { has_admin: bool }

async fn fetch_init(base: &str) -> Result<InitInfo, reqwest::Error> {
    let url = format!("{base}/admin/init");
    reqwest::get(&url).await?.json::<InitInfo>().await
}
```

- Web: use a relative base URL (`""`) or the dev server origin so CORS is not a problem;
  configure `tower-http::cors` on Axum when the client origin differs.
- Desktop: point the client at `http://127.0.0.1:PORT` where the Axum server listens.

## 6. State and async

- Use `use_signal` / `Signal` for state, and `spawn` for async work inside components.
- Do not block the render loop; always await network calls in a spawned task.
- Store the JWT from the backend in a signal (or `use_context`) and send it as a
  `Bearer` header on subsequent requests.

```rust
#[component]
fn Login() -> Element {
    let mut email = use_signal(String::new);
    let mut pw = use_signal(String::new);
    let mut err = use_signal(|| None::<String>);
    rsx! {
        input { value: "{email}", oninput: move |e| email.set(e.value()) }
        input { value: "{pw}", oninput: move |e| pw.set(e.value()), r#type: "password" }
        button { onclick: move |_| { let e = email(); let p = pw(); spawn(async move { /* call api */ }) } }
    }
}
```

## 7. Design tokens & styling

Keep design tokens (colors, spacing, typography) in one module and reference them in
inline `style:` attributes so the same tokens drive both web and desktop. Example:

```rust
const PRIMARY_600: &str = "#4945FF";

rsx! {
    div { style: "background:{PRIMARY_600}; padding:16px; border-radius:4px;",
        "content"
    }
}
```

## 8. Routing (optional)

If the scaffold included the router, use `dioxus-router`:

```rust
#[component]
fn App() -> Element {
    rsx! {
        Router::<Route> {}
    }
}

#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[route("/")]
    Home {},
    #[route("/login")]
    Login {},
}
```

## 9. Checklist before finishing

- [ ] Scaffolded with `dx new` (or matched its layout: `Cargo.toml`, `Dioxus.toml`, `assets/`, `src/main.rs`).
- [ ] `dx build --web` compiles (wasm32) with no errors.
- [ ] `dx build --desktop` (or the desktop feature) compiles natively with no errors.
- [ ] Client talks to the Axum backend via JSON; auth token flows as a Bearer header.
- [ ] `cargo check --workspace` passes (server + client in the same workspace).
- [ ] No render-loop blocking; all network calls are spawned.
