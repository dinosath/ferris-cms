# Ferris Application Platform — Implementation Plan

This document turns the **vision** in [`plan2.md`](plan2.md) into a **concrete,
dependency-ordered engineering plan** grounded in the current Ferris CMS
codebase. It is the execution counterpart to plan2.md: where plan2.md says
"what", this says "which crate, which module, in which order, and what proves
it works".

It assumes the reader knows the workspace layout in
[`docs/IMPLEMENTATION.md`](docs/IMPLEMENTATION.md) and the canonical crate map
below.

---

## 0. Current state vs. the plan2 vision

Before any new work, we must know what already exists. The current repo is a
Strapi-compatible headless CMS and already covers a meaningful slice of plan2:

| plan2 capability | Current state | Gap |
|---|---|---|
| Content types / schema model | `core-schema::Schema/Attribute`, validation, `SchemaDiff` | Is the *single* schema. Needs to become one node in a broader Application Model. |
| Runtime DDL / migrations | `dynamic-store::ddl` applies diffs via SeaQuery | Ad-hoc, immediate apply. Needs explicit **generated migration files**, plan/apply/verify/rollback, never-silent production changes. |
| Resource CRUD + dynamic queries | `dynamic-store` DML + `api-rest` `/api/*` | Queries are built per-call; no abstract query IR, no inspector/DataLoader batching. |
| Workflows | `workflow` domain + `services/workflow` engine (triggers, executors, credentials) | No durable execution, no retry/idempotency/dead-letter, no compensation, no Temporal-style pluggable backend. |
| RBAC / permissions | `services::rbac` + `db` permission entities | Role/action matrix only. No ABAC/policies, row/field/interface-level, no permission debugger. |
| Auth | `services::auth` JWT + argon2, admin + API tokens | No OAuth/OIDC/SSO/service accounts. Auth is coupled to authorization. |
| AI | `services::ai` (providers, tools, chat, security, usage) | No model-first codegen/interface/workflow generation, no slow-query diagnosis. |
| Import/export | `services::import_export/*` | Content-type JSON only. No full project export, no `ferris diff`. |
| UI | Dioxus `app` + `ui` design system | Content Manager + CTB only. No Grid/Form/Kanban/Calendar/Gallery/Dashboard interfaces, no query inspector. |
| Datasources | Postgres + SQLite only, hard-coded | No datasource abstraction, no introspection, no REST/GraphQL/CSV/JSON adapters, no external/virtual/synced modes. |
| Code generation | `docs/09` design intent only, **no `codegen` crate** | Entire compiler, Kubernetes generator, observability-in-generated-code, safe-regeneration separation. |
| Deployment | `Dockerfile`, `docker-compose.yml`, `deploy/helm/ferriscms` | Existing infra is for the *CMS itself*, not generated apps. Needs generated-app deployment targets. |

**Key architectural gap**: today `Schema` == the whole world. plan2 requires a
wider, versioned, Git-friendly **Application Model** that *contains* resources,
datasources, interfaces, workflows, policies, auth, integrations, jobs, and env
config — with `Schema` (or a new `Resource` model) as a component of it.

---

## 1. Guiding constraints (must hold for every phase)

1. **Model first, UI second.** The Application Model is the source of truth.
   Studio manipulates the model, never only UI metadata.
2. **No lock-in.** Every artifact has a portable, text-based representation and
   can be exported. Generated apps run without Ferris.
3. **Database transparency.** Never hide or auto-apply schema changes silently.
   Expose PKs, FKs, constraints, indexes, relations, transactions, migrations.
4. **One security model.** Authorization is identical across UI, API,
   workflows, generated apps, and integrations.
5. **Acyclic dependency direction.** `core-domain` ← `core-schema` ←
   `dynamic-store`/`services` ← `api-rest` ← binaries. New crates must preserve
   this. `ui`/`app` never import `services`/`db`/`dynamic-store`.
6. **Safe code generation.** Generated code and user code are separated
   (`generated/` vs `app/` + `extensions/`); regeneration touches only
   `generated/`. Generation is deterministic.
7. **Incremental, not a rewrite.** Preserve the existing CMS as a running
   system; migrate it into the platform (plan2 §34).

---

## 2. Sequencing at a glance

The phases below are ordered so each is **shippable** and unblocks the next.
Arrows = hard dependency.

```
P1 Model Core → P2 Studio on Model → P3 Datasources → P4 Query Engine
     │                                     │                │
     ├─ P6 CMS + Migration Engine          └──► P5 Interfaces
     │                                             │
     ├─ P7 Authz+Auth   ──►  P8 Integrations  ──►  P9 Workflow Reliability
     │                                                  │
     ├─ P10 Codegen (Rust)  ──►  P11 Obs + K8s Gen  ──►  P12 Deploy Targets
     │                                                   │
     └─ P13 Versioning+Diff+Import  ──►  P14 AI on Model  ──►  P15 E2E+SAAS+Multi
```

Legend: the four **structural pillars** are P1 (model), P3 (datasources),
P4 (query), P10 (codegen). Everything else hangs off them.

---

## 3. Phase 1 — Application Model (foundation)

**Goal**: a versioned, Git-friendly model that is the single source of truth.

### 3.1 New crate `crates/application-model`
Pure domain types (no I/O), like `core-domain` today. Define:
- `App`, `AppVersion`, `Environment`, `Dependency`, `Module`.
- `Resource` (generalizes Strapi content type + NocoDB table + external
  resource): `name`, `source` (datasource ref), `table`, `capabilities`
  (create/read/update/delete/bulk/transactions/subscriptions/search/filter/
  aggregate/sync/draft-publish/versioning/localization/audit).
- `Field` (superset of `core-schema::Attribute`): add `computed`, `formula`,
  `indexing`, `permissions`, `conditional` to the existing set; keep
  `core-domain::FieldType` as the type taxonomy.
- `Relation` (one-to-one/many/many-to-one/many-to-many/self/polymorphic).
- `DatasourceDecl`, `Interface`, `View`, `Workflow`, `Policy`, `AuthConfig`,
  `Integration`, `Job`, `EnvConfig`, `DeploymentConfig`.
- A top-level `ApplicationModel { version, resources, datasources, interfaces,
  workflows, policies, ... }` that (de)serializes to the plan2 §16 project
  layout.

### 3.2 Relationship to existing schema
- Keep `core-schema` as the *Resource field/schema engine*; do **not** fork it.
- Add an adapter so `ApplicationModel::Resource` maps to/from
  `core-schema::Schema` (bidirectional, lossless). This is how existing CMS
  content types become resources (plan2 §34 Step 1–2).

### 3.3 Docs (plan2 §4 deliverables)
Write into `docs/`: `architecture.md`, `application-model.md`,
`datasource-model.md`, `workflow-model.md`, `interface-model.md`,
`authorization-model.md`, `code-generation.md`, `compatibility.md`. These are
the contract before implementation of dependent phases.

### Acceptance
- `ApplicationModel` round-trips YAML/JSON losslessly; fixtures cover every
  artifact type.
- A `Resource` ⇄ `Schema` mapping test passes for all existing CT fixtures.
- `ferris validate` rejects malformed models with actionable errors.

---

## 4. Phase 2 — Studio on the model (model-first UI)

**Goal**: the Studio edits the Application Model, not ad-hoc state.

- Refactor the existing Content-Type Builder UI (`app`, `ui`) to read/write a
  `Resource` working-copy on top of `ApplicationModel`, replacing direct
  schema-JSON manipulation with model operations (keep the same UX; change the
  storage substrate). Reuse the existing unsaved-changes/undo-redo machinery.
- Add model-scoped navigation: Datasources, Resources, Interfaces, Workflows,
  Policies, Integrations, Settings tabs (new screens; CTB becomes a screen).
- Preview pane renders the current model (no execution yet).

### Acceptance
- Editing a resource in the Studio produces a valid `ApplicationModel` delta.
- `cargo test --workspace` stays green; existing CTB flows unchanged end-to-end.

---

## 5. Phase 3 — Datasource system

**Goal**: Ferris's key differentiator — a stable datasource abstraction.

### 5.1 New crate `crates/datasource`
- `trait Datasource` with capability discovery: `list_schemas`, `introspect`
  (tables, columns, types, PKs, FKs, indexes, constraints, relations),
  `query(QueryPlan)`, `mutate(Mutation)`, `transact`, `native_sql`.
- Adapters (initial): `Postgres`, `MySQL`, `SQLite`, `Rest`, `GraphQL`,
  `Csv`, `Json`. Future: MSSQL, Mongo, S3, Kafka, GitHub, ERP, CRM.
- Reuse existing `db`/`dynamic-store` Postgres+SQLite code behind the trait;
  do not duplicate.

### 5.2 Introspection → resource generation
- Implement the plan2 §6.2 pipeline: connect → inspect → select resources →
  generate `Resource` declarations into the model.
- This is the **first reverse-engineering** capability (plan2 §19) and the
  bridge to external/read-only/synced/virtual resource modes (plan2 §6.3).

### Acceptance
- `ferris datasource connect` + `introspect` on a Postgres DB produces
  `Resource` YAML with correct types, PK/FK, and relations.
- CSV/JSON adapters materialize read-only resources; REST adapter queries an
  OpenAPI-backed service.

---

## 6. Phase 4 — Query engine

**Goal**: kill the N+1 problem and give perf transparency.

- New `crates/query` (pure): `Query { fields, filters, joins, sort, pagination,
  aggregation, permissions }` → compiled to per-datasource SQL via existing
  SeaQuery/`dynamic-store`, with **DataLoader/batching** for relation
  expansion, query caching, lazy relation loading, complexity limits, timeouts.
- `QueryInspector` returns: generated query, execution time, rows scanned,
  rows returned, indexes used, warnings (plan2 §9). Wire into the Studio
  (query inspector panel) and the REST layer.
- Replace uncontrolled per-row populate in `services::content` with the query
  engine (the biggest correctness/perf win available today).

### Acceptance
- A Grid query with a populated relation issues ≤ 2 SQL statements regardless
  of row count (DataLoader proof), verified by a test asserting query count.
- Inspector output is exposed via a dev/admin endpoint and rendered in the UI.

---

## 7. Phase 5 — Interfaces (NocoDB-style)

**Goal**: interfaces become first-class application artifacts rendered from the
model + query engine.

- New `Interface` artifacts in the model; a generic `Grid` first
  (sort/filter/group/paginate/columns/inline+bulk edit/relation expansion/
  saved views/formulas/aggregations), reusing the existing table + design
  system widgets.
- Then `Form`, `Detail`, `Kanban`, `Calendar`, `Gallery`, `Timeline`,
  `Dashboard`, `Master/detail`, `Custom page` — each reads a `Query` + applies
  `Policy` (permission-aware rendering).
- Interface-level permissions gate visibility of grids, forms, delete, fields,
  and actions (plan2 §8 example).

### Acceptance
- A `Grid` + `Form` on a resource render from the model with working CRUD.
- The same interface shows/hides the "Cost" field and the "Delete" action
  per role, matching the plan2 §8 example.

---

## 8. Phase 6 — CMS capabilities + migration engine

**Goal**: solve the Strapi pain points plan2 targets (draft/publish depth,
versions, localization, media, and trustworthy migrations).

- Content features (plan2 §10): draft/publish, versions + revision history,
  localization, media library, components + repeatables + dynamic zones
  (partially exist), hierarchical content, scheduled publishing, preview,
  editorial workflow with explicit state machines. Keep lifecycle **out** of
  the DB layer; model it as workflows/state machines.
- Migration engine (plan2 §3): declarative schema diff → *generated migration
  files* → `plan / generate / apply / verify / rollback` (where safe) → seed +
  data migrations → index/constraint management. Extend `dynamic-store::ddl`
  to emit migration artifacts instead of applying DDL blindly.
- CLI: `ferris migrate plan|generate|apply|verify`. **Never** silently mutate
  production; require an explicit plan review path.

### Acceptance
- Editing a resource emits a human-readable migration file (ADD/ALTER/DROP
  listing, plan2 §3) and applies it only on `apply`.
- A draft/publish + scheduled-publish test passes against the workflow engine.

---

## 9. Phase 7 — Authorization + authentication

**Goal**: one unified security model across UI, API, workflows, and generated
apps.

- Extend `services::rbac` with a **Policy engine** (`crates/policy`, pure):
  resource/field/record/interface/workflow/action/API policies with
  `allow`/`deny` rules (plan2 §8), reusable predicate evaluator shared by
  queries (row-level), UI, and workflows.
- **Permission debugger**: given user+role+action+record, explain allow/deny
  and the actual evaluated values (plan2 §8) in dev/admin mode.
- Separate **authentication** (`services::auth`) from **authorization** so
  generated apps can swap auth providers without touching the model. Add
  OAuth/OIDC, SSO, API keys, service accounts, m2m (plan2 §13).

### Acceptance
- The plan2 §8 "Why can't this user edit Invoice #182?" scenario is answered
  correctly by the debugger, including the `17 != 23` mismatch.
- Same policy result is produced from API, UI, and a workflow execution.

---

## 10. Phase 8 — Integrations framework

**Goal**: integrations expose capabilities, usable from workflows, interfaces,
backend, and generated apps.

- New `crates/integration`: registry of capability providers (Stripe, email,
  Slack, GitHub, S3, webhooks, REST, GraphQL, ERP, CRM) described as typed
  capability manifests (e.g. `stripe.customers.read`).
- Wire the existing `services/workflow/executors` to call integrations through
  the registry (replacing ad-hoc node executors where possible).

### Acceptance
- An `httpRequest`-style workflow node and an email node resolve through the
  integration registry; capabilities are discoverable in the Studio.

---

## 11. Phase 9 — Workflow reliability + durability

**Goal**: production-grade workflows (plan2 §7).

- Add to the existing engine: retries, idempotency, timeouts, dead-letter
  handling, durable execution (event-sourced execution state), compensation,
  concurrency limits.
- Keep workflow *definitions* stable so a future Temporal-style backend can
  replace the executor without model changes (abstract an `ExecutionBackend`
  trait behind `services::workflow`).

### Acceptance
- A workflow that throws after `N` records restarts and completes without
  duplicate side effects (idempotency test).
- Failure lands in a dead-letter queue and is visible in the Studio.

---

## 12. Phase 10 — Rust code generator (the core product feature)

**Goal**: compile the model into a standalone Rust/Axum/Postgres application.

- New crate `crates/codegen`: model → source tree
  (`src/main.rs`, `api/`, `domain/`, `repository/`, `services/`, `workflows/`,
  `auth/`, `generated/`, `extensions/`, `migrations/`, `tests/`,
  `Dockerfile`, `compose.yaml`, `helm/`, `README.md`).
- Generate from the model's resources, relations, policies, workflows,
  auth config, and query definitions. Output is idiomatic, formatted
  (`rustfmt`), modular, independently buildable, and **deterministic**
  (same model → same bytes; golden tests).
- Safe separation (plan2 §18): `src/generated/` regenerates; `src/app/`,
  `src/extensions/` preserved. Extension points let generated code call
  user code (plan2 §17).
- Emit migrations from the P6 migration artifacts, not ad-hoc DDL.

### Acceptance
- `ferris generate --target rust-axum` on a sample CRM model produces a
  project that compiles, passes its generated tests, and runs against Postgres
  (validated in CI on a real DB).
- Regeneration does not touch user edits in `src/app/`/`src/extensions/`.

---

## 13. Phase 11 — Observability + Kubernetes generator in generated apps

**Goal**: production-grade output (plan2 §14–§15).

- Generated apps embed `tracing`/`tracing-subscriber`, OpenTelemetry
  (traces/metrics/logs), `metrics` (HTTP count/latency/errors, DB latency/pool,
  workflow executions/failures, queue depth), and `/health` + `/ready`
  K8s-compatible endpoints.
- `crates/codegen` additionally emits a Helm chart (`Chart.yaml`, `values.yaml`,
  `templates/{deployment,service,ingress,configmap,secret,hpa,pdb,
  servicemonitor,networkpolicy}.yaml`) + `Dockerfile` + `compose.yaml` with
  sane, overridable defaults.

### Acceptance
- A generated app reports `/health`/`/ready` correctly and emits OTLP traces +
  metrics (validated with an in-test OTLP collector).
- `helm template` on a generated chart renders all resources without error.

---

## 14. Phase 12 — Deployment targets

**Goal**: generated infra remains editable and targets real environments.

- Targets: Docker Compose (default), Docker, Kubernetes/Helm; then ArgoCD, and
  environment config as generated `ConfigMap`/env (secrets never committed,
  plan2 §33).
- Wire `ferris deploy` to the chosen target.

### Acceptance
- `docker compose up` on a generated app starts the app + Postgres and passes a
  smoke request; `ferris deploy --target kubernetes` renders the chart.

---

## 15. Phase 13 — Versioning, diff, import/export, reverse-engineering

**Goal**: Git-native model lifecycle (plan2 §12, §29, §19, §31).

- Version every model component (`Resource v4`, `Workflow v7`, …); extend the
  existing `services/import_export` into full **project export/import** using
  the plan2 §16 `my-app/` layout.
- `ferris diff` produces ADD/MODIFY/REMOVE on resources, fields, indexes,
  workflows (integrates with Git).
- Reverse-engineering: existing Postgres → introspect → model (P3), existing
  API → OpenAPI → datasource, and (future) existing Rust app → model.

### Acceptance
- `ferris export` of a modeled app is a valid Git repo you can `import` back
  byte-identically; `ferris diff` between two model versions lists the right
  deltas.

---

## 16. Phase 14 — AI layer on the model

**Goal**: AI operates against the model + authorization (plan2 §11).

- Extend `services::ai` with model-aware tools: generate resource/fields/
  relations/interfaces/workflows, explain permissions/schema, generate
  queries/migrations/Rust code/tests, diagnose slow queries (reads the query
  inspector).
- Enforce: AI never bypasses policies; every generated action is evaluated as
  user + role + resource + action + policy; inaccessible records are not
  revealed to the assistant.

### Acceptance
- "Create a CRM for sales representatives" yields a valid model with the plan2
  §11 resources, interfaces, and a Deal-Won→Invoice workflow.
- An AI action that a role cannot perform is rejected by the policy engine.

---

## 17. Phase 15 — Testing, E2E, observability of Ferris itself, SaaS, multi-tenancy

- **Testing at every layer** (plan2 §20): model validation/compat/migration
  tests, datasource integration tests (Postgres/MySQL/API adapters), interface
  component/a11y/interaction tests, deterministic workflow tests (retry/
  failure/concurrency), authz `role×action×resource×record×field` matrix,
  and every generated app compiles + passes generated tests.
- **E2E** (plan2 §21): full scenario create→resource→interface→workflow→
  permissions→generate→Docker→Postgres→run→API→workflow→telemetry; screenshot
  regression for Studio.
- **Ferris self-observability** (plan2 §22): apply the same tracing/metrics
  principles to Ferris itself (API/DB/datasource/workflow/codegen/deploy
  latency, failed migrations/workflows, queue depth, active users).
- **SaaS control plane + multi-tenancy** (plan2 §27–§28): separate control
  plane from customer apps; tenant isolation (shared-DB `tenant_id`, separate
  schema, or separate DB) designed in from the start, with the model not
  assuming one strategy.

### Acceptance
- Full E2E scenario passes in CI against real Postgres.
- A multi-tenant app isolates two tenants' data under all three strategies.

---

## 18. Phase 16 — DX, CLI, marketplace

- Full CLI (plan2 §31–§32): `init`, `dev` (Studio + API + worker + Postgres +
  observability in one command), `validate`, `diff`, `migrate`, `generate`,
  `build`, `test`, `export`, `deploy`, with deterministic `--target`,
  `--database`, `--observability`, `--deployment` flags.
- Extension/marketplace model (plan2 §30): typed extension categories
  (datasource, field, interface, workflow node, auth provider, integration,
  code generator, deployment target, theme, AI provider) with a `requires:
  ferris: ">=1.4 <2"` compatibility declaration; extensions never touch
  internal implementation details.

---

## 19. Cross-cutting priorities

1. **Ship the four pillars first**: P1 model, P3 datasources, P4 query,
   P10 codegen. Everything else is leverage on these.
2. **Migration from existing CMS** (plan2 §34) is a side-track that runs
   through every phase: existing content types, fields, relations, workflows,
   permissions, and admin screens map onto model artifacts as the model grows.
3. **Reuse over rewrite.** `core-schema`, `dynamic-store`, `services/*`,
   `services/ai`, `services/import_export`, the `ui` design system, and the
   `app` Studio are the raw material. New crates (`application-model`,
   `datasource`, `query`, `policy`, `integration`, `codegen`) wrap or extend
   them behind clean interfaces; never duplicate.

---

## 20. Suggested immediate first work items

Ordered by dependency, each independently committable (conventional-commit
style, `cargo test --workspace` green):

1. **P1** — add `crates/application-model` with `Resource`, `Field`,
   `Relation`, `DatasourceDecl`, and the `ApplicationModel` container; a
   `Resource` ⇄ `core-schema::Schema` adapter; docs for `application-model.md`.
2. **P1** — versioned model (per-component versions + `AppVersion`).
3. **P3** — `crates/datasource` trait + Postgres/SQLite adapters (reusing
   `db`), introspection returning resource drafts.
4. **P4** — `crates/query` IR + DataLoader batching in `services::content`.
5. **P10** — `crates/codegen` scaffold producing a hello-world Rust/Axum app
   from an empty model, establishing the deterministic-output harness that
   later phases fill in.

---

## 21. Acceptance bar for the plan as a whole

The plan is "done" when:

- The existing CMS runs unchanged **and** every plan2 capability is reachable
  through the Application Model.
- `ferris generate --target rust-axum` produces a standalone, compilable,
  tested, observable, deployable application from a model built in the Studio.
- Authorization is provably identical across UI, API, workflows, and generated
  apps.
- Full E2E (build → generate → Docker → Postgres → run → API → workflow →
  telemetry) passes in CI.
