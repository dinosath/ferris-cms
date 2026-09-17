# ferriscms

A **1:1, offline-first clone of [Strapi](https://github.com/strapi/strapi)** built entirely in **Rust**.

ferriscms reproduces Strapi's core headless-CMS workflow with a single Rust
codebase that runs in two modes:

- **Offline desktop** — an embedded SQLite database, no server, no config.
- **Online server** — an Axum server (SQLite by default, PostgreSQL when
  `DATABASE_URL` says so) serving a Strapi-compatible REST API plus the
  Dioxus admin UI.

```
┌─────────────────────────────────────────────────────────────┐
│                     shared library crates                    │
│   core-domain, core-schema, db, dynamic-store, services,     │
│   api-types, client-core                                     │
└──────────────▲──────────────────────────────▲───────────────┘
               │ in-process calls              │ HTTP
   ┌───────────┴──────────┐        ┌───────────┴─────────────┐
   │  desktop-bin (OFFLINE)│        │   server-bin (ONLINE)   │
   │  Dioxus UI ─ client-core     │   Axum ─ services        │
   │  services + SQLite (embedded)│   Postgres / SQLite       │
   └────────────────────────┘        └───────────┬─────────────┘
                                                │ HTTP
                                    ┌───────────┴─────────────┐
                                    │  Web admin (Dioxus WASM) │
                                    │  client-core (HTTP)      │
                                    └─────────────────────────┘
```

---

## Features

The project implements the "core" of Strapi (no code generation, plugin
system, or GraphQL yet):

- **Content-Type Builder** — visually define collection types, single types,
  components, and dynamic zones at runtime. Field types include text, rich
  text (Markdown/Blocks), number, date, boolean, email, password, enum,
  JSON, UID, media, relation, component, and dynamic zone. All six relation
  kinds, `New / Modified / Deleted` staging badges, undo/redo/discard.
- **Content Manager** — schema-driven entry forms, list/edit views, filters,
  sorting, pagination, **Draft & Publish**, and discard-draft controls.
- **Dynamic schema → real tables** — schemas are stored as JSON and applied to
  the database at runtime (SeaQuery-generated DDL). No manual SQL.
- **Strapi-compatible REST API** — `filters`, `populate`, `sort`,
  `pagination`, `fields`, `locale`, and `status` query params, with Strapi's
  `{ data, meta }` envelope shape.
- **Media Library** — upload files, list/delete, served from `/uploads`.
- **RBAC** — roles and a permission matrix (Super Admin, Editor, Author seeded).
- **i18n** — locale registry and localized content variants.
- **API Tokens** — read-only/full/custom tokens for the public API.
- **JWT admin auth** — first-run registration + login (argon2 password hashing).

---

## Tech stack

| Layer | Technology |
|---|---|
| Language | Rust (edition 2021) |
| Web server | [Axum](https://docs.rs/axum) 0.8 |
| ORM / query builder | [SeaORM](https://www.sea-ql.org) 2.0 + [SeaQuery](https://docs.rs/sea-query) 1.0 |
| Migrations | `sea-orm-migration` (system) + runtime DDL (user content-types) |
| Database | SQLite (default, embedded) / PostgreSQL 14+ (`DATABASE_URL`, online) |
| UI | [Dioxus](https://dioxuslabs.com) 0.7 (native desktop + WASM web) |
| Async runtime | Tokio |
| Auth | JWT (`jsonwebtoken`) + `argon2` password hashing |
| Serialization | `serde` / `serde_json` |

---

## Project layout

A Cargo workspace of library crates plus two thin binary crates:

```
ferriscms/
├─ Cargo.toml                  # workspace manifest + shared dependencies
├─ docs/                       # planning / design docs (see below)
├─ crates/
│  ├─ core-domain/             # pure domain types (no IO): kinds, fields, relations, UID
│  ├─ core-schema/             # schema model, JSON (de)serialization, validation, diffing
│  ├─ api-types/               # shared request/response DTOs + Strapi query parser
│  ├─ db/                      # SeaORM 2.0 system entities, migrations, seeding
│  ├─ dynamic-store/           # runtime DDL + CRUD for user-defined content-types
│  ├─ services/                # business logic (CTB, content, media, rbac, i18n, auth)
│  ├─ api-rest/                # Axum routers: /api, /admin, /content-type-builder
│  ├─ client-core/             # transport-abstract client the UI calls
│  ├─ ui/                      # design system: tokens, widgets, screens (framework-agnostic)
│  ├─ app/                     # Dioxus admin UI (web + desktop), calls client-core
│  ├─ server-bin/              # binary: online Axum server (ferriscms)
│  └─ desktop-bin/             # binary: offline desktop + embedded server (ferriscms-desktop)
```

---

## Installation

### Prerequisites

- **Rust** — the workspace targets Rust edition 2021; the docs assume
  **MSRV 1.82+**. Installed with [rustup](https://rustup.rs).
- **Dioxus CLI (`dx`)** — only needed for the web/desktop **UI** build:
  ```bash
  cargo install dioxus-cli
  ```
- **PostgreSQL** — *optional*. The webserver (`server-bin`) defaults to a
  local SQLite file (`ferriscms.db`); set `DATABASE_URL` to a `postgres://…`
  URL to run it on PostgreSQL instead. The desktop app always uses embedded
  SQLite.

### Build

Build the entire workspace (backend + UI libraries):

```bash
cargo build --workspace
```

Build just the two runtime binaries:

```bash
cargo build -p server-bin -p desktop-bin
```

Build the Dioxus admin UI (the `ferriscms` app crate). The `dx` CLI needs to
locate the app's `Dioxus.toml`, which lives in `crates/app/`. Either run from
inside that directory, or point `dx` at the package with `--package ferriscms`:

```bash
# native desktop app
cd crates/app
dx build --desktop --features desktop

# web (WASM) bundle -> crates/app/target/dx/ferriscms/debug/web
cd crates/app
dx build --web

# ...or, from the workspace root, use the package flag:
dx build --web --package ferriscms
```

### Test

```bash
cargo test --workspace
```

Tests include a full end-to-end admin workflow against the Axum router
in-memory (`crates/api-rest/tests/auth_workflow.rs`) and dynamic-store CRUD
integration tests.

---

## Running

### Online server (`ferriscms`)

Starts the Axum REST API + admin API on the configured port, running
migrations and seeding roles/locales on boot. With no configuration it uses a
local **SQLite** file (`ferriscms.db` in the working directory, gitignored), so
`cargo run -p server-bin` works out of the box; set `DATABASE_URL` to a
`postgres://…` URL to use **PostgreSQL**.

```bash
cargo run -p server-bin
```

| Env var | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | `sqlite://ferriscms.db?mode=rwc` | Local SQLite file by default; set a `postgres://…` URL for PostgreSQL |
| `BIND_ADDR` | `0.0.0.0:8080` | HTTP listen address |
| `JWT_SECRET` | `change-me-in-production` | HS256 signing secret (set in production!) |
| `MEDIA_STORAGE_DIR` | `media` | Directory for uploaded files |

On first boot, register the first admin via `POST /admin/register-admin`
(or use the UI). Then log in through `POST /admin/login` to get a JWT.

### Local development

Run the backend and the web UI together with cargo-make. The `--no-workspace`
flag prevents cargo-make from trying to run the task in every workspace crate.
The backend listens on port `8080`; the Dioxus web server uses port `8081`.
In development mode, the migration creates `admin` / `admin` as a local
Super Admin.

Install [cargo-make](https://github.com/sagiegurari/cargo-make) if it is not
already available:

```bash
cargo install cargo-make
```

```bash
cargo make --no-workspace dev
```

Without cargo-make, use two terminals from the repository root:

```bash
# Terminal 1: Axum backend
FERRISCMS_ENV=development cargo run
```

```bash
# Terminal 2: Dioxus web UI
cd crates/app
FERRISCMS_API_URL=http://127.0.0.1:8080 dx serve --web --package ferriscms --port 8081
```

The `dx` command requires the Dioxus CLI version matching the app dependency
(currently `0.7.10`). The web UI is available at `http://127.0.0.1:8081` and
the backend at `http://127.0.0.1:8080`.

### Offline desktop (`ferriscms-desktop`)

Runs the full backend (SQLite + migrations + seed) and exposes the REST API
on a local HTTP port — no external database or server required.

```bash
cargo run -p desktop-bin
```

| Env var | Default | Purpose |
|---|---|---|
| `STRAPI_DB_PATH` | `ferriscms-desktop.db` | Path to the local SQLite file |
| `STRAPI_BIND_ADDR` | `127.0.0.1:1338` | Local HTTP listen address |
| `JWT_SECRET` | `desktop-local-dev` | Signing secret |
| `MEDIA_STORAGE_DIR` | `media` | Upload directory |

### Admin UI

The Dioxus admin UI (the `ferriscms` app crate) talks to the backend through
`client-core`. On **web** it uses same-origin requests; on **desktop/native**
it targets `FERRISCMS_API_URL` (default `http://127.0.0.1:1337`).

Because the app's `Dioxus.toml` lives in `crates/app/`, run `dx` from inside
that directory and select the app package explicitly:

```bash
# Dev server (hot reload) for the web UI
cd crates/app
FERRISCMS_API_URL=http://127.0.0.1:8080 dx serve --web --package ferriscms --port 8081

# Native desktop app
cd crates/app
dx run --desktop --features desktop
# or, from the workspace root:
dx run --desktop --package ferriscms --features desktop

# Point a desktop/native build at a running server
FERRISCMS_API_URL=http://127.0.0.1:1337 ./target/debug/ferriscms
```

> The web build is a separate static bundle. Run it alongside the server
> (`cargo run -p server-bin`) so the UI can reach the API, or host the
> `crates/app/target/dx/ferriscms/debug/web` output on any static server that
> proxies `/api`, `/admin`, and `/content-type-builder` to the Axum process.

---

## Container & Helm deployment

The repo ships a `Dockerfile` for the server binary and a Helm chart so the
CMS can be deployed to Kubernetes.

### Docker image

Build the `ferriscms` image locally. The build produces a **single
self-contained binary** that embeds both the Axum webserver and the Dioxus WASM
admin UI (via `rust-embed`):

```bash
docker build -t ferriscms .
```

The image is the webserver and uses **PostgreSQL** (external). The one binary
serves the admin UI at `/` and the REST API at `/api`, `/admin`, and
`/content-type-builder`. Media is stored in `/data/media`. Set the DB
connection at run time:

```bash
docker run --rm -p 1337:1337 \
  -e DATABASE_URL='postgres://user:pass@host:5432/ferriscms' \
  -e JWT_SECRET='a-strong-secret' \
  -e MEDIA_STORAGE_DIR=/data/media \
  -v ferriscms-media:/data/media \
  ferriscms
```

For development, you can serve the UI from a directory instead of the embedded
copy by setting `FERRISCMS_UI_DIR` (e.g. point it at a `dx serve` output).

### Helm chart

The chart lives in [`deploy/helm/ferriscms/`](deploy/helm/ferriscms/):

```bash
helm lint deploy/helm/ferriscms
helm template ferriscms deploy/helm/ferriscms
helm install ferriscms deploy/helm/ferriscms \
  --set image.tag=0.2.0 \
  --set env.JWT_SECRET='a-strong-secret'
```

Useful `--set` overrides:

| Value | Default | Purpose |
|---|---|---|
| `image.tag` | `appVersion` | Image tag to deploy |
| `env.DATABASE_URL` | `postgres://postgres:postgres@postgres:5432/ferriscms` | PostgreSQL connection URL |
| `env.JWT_SECRET` | `change-me-in-production` | Signing secret — set in production! |
| `persistence.enabled` | `true` | Mount a PVC at `/data` for media |
| `persistence.size` | `1Gi` | PVC size |

### Docker Compose

A [`docker-compose.yml`](docker-compose.yml) starts a PostgreSQL database plus
the webserver, using profiles:

```bash
# Build the image from the local Dockerfile (dev)
docker compose --profile local up --build

# Pull and run the prebuilt image from GHCR (registry)
docker compose --profile registry up
```

Both profiles share the same Postgres database and expose ferriscms on
`localhost:1337`. Set `JWT_SECRET` (and optionally `POSTGRES_USER`,
`POSTGRES_PASSWORD`, `POSTGRES_DB`) via a `.env` file. For the registry
profile, override the image with `FERRISCMS_IMAGE=ghcr.io/dinosath/ferris-cms:<tag>`.

---

## License

[MIT](LICENSE)
