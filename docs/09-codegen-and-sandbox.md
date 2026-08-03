# 09 — Codegen, Sandboxing & the "Eject" Pipeline (Decision Record)

Status: **Accepted** · Date: 2026-07-30 · Supersedes: none · Related: [01-architecture.md](01-architecture.md), [02-data-model.md](02-data-model.md), [03-content-type-builder-logic.md](03-content-type-builder-logic.md), [07-offline-sync.md](07-offline-sync.md)

This document records the decision about **how content-type APIs are executed**: a dynamic runtime engine vs. generated-and-compiled Rust code, and where (if anywhere) code generation, sandboxing, and git fit in. It applies equally to offline (`desktop-bin` + SQLite) and online (`server-bin` + Postgres) modes.

---

## 1. Context & the question

Strapi lets admins define content-types at runtime and then exposes a CRUD REST API per type. Two ways to realize this in Rust:

- **A. Dynamic engine** — store the schema as JSON; build SQL at runtime (SeaQuery). No codegen, no compile, no restart.
- **B. Codegen + compile** — render Rust source (entities, routes, handlers) from the schema via templates (Baker/MiniJinja), compile it (in a Docker/K8s/WASM sandbox), commit to git, and load it behind the Content Manager "gateway".

Key observation that drives the decision: **Strapi never compiles.** Its generated `src/api/<name>/{controllers,routes,services}` files are thin boilerplate that delegate to generic factories (`createCoreController`, etc.); `schema.json` is the real driver, and Node interprets everything with no build step. Therefore, for a **1:1 clone**, codegen is *not required for parity* — a dynamic engine already reproduces Strapi's runtime behavior. Codegen is a **capability beyond Strapi**, not a prerequisite.

A second observation: **Rust is compiled, JavaScript is not.** This makes approach B far more expensive for us than it is for Strapi (toolchain, build latency, restart/hot-load), and it is especially hostile to the offline-first requirement (shipping `rustc`/`cargo` in a desktop app and forcing a `cargo build` on every field edit destroys the "instant" UX).

---

## 2. Decision

Adopt a **three-layer model**. The dynamic engine is the runtime spine; sandboxed scripting provides custom logic; code generation is an **opt-in "eject/export" feature**, never on the request hot path.

| Layer | Role | Runtime location | Codegen? | Compile? |
|---|---|---|---|---|
| **L1 — Dynamic CRUD engine** | Serves all CRUD for every content-type | in-process (both modes) | no | no |
| **L2 — Scriptable hooks & policies** | Custom business logic (lifecycle hooks, policies, validators) | sandboxed, in-process (both modes) | no (author scripts) | no |
| **L3 — Eject / export codegen** | Generate a standalone Rust project the user owns/deploys | offline: on disk; online: Docker build | **yes (Baker/MiniJinja)** | yes (server-side or by the user) |

**One-line stance:** do not compile per-content-type at runtime; run the dynamic engine + sandboxed scripts, and treat Baker/MiniJinja codegen as an opt-in "eject to a real Rust project" feature backed by git and (server-side) Docker builds.

---

## 3. Untangling "sandbox" — three distinct jobs

The word "sandbox" conflated three unrelated concerns. They have different answers:

| Job | What it needs | Verdict |
|---|---|---|
| **Generate code** (text templating) | CPU only; MiniJinja | **In-process. No container, no sandbox.** |
| **Build code** (compile Rust) | `rustc` + `cargo`, heavy | **Docker, server-side only.** Kubernetes **only** if multi-tenant SaaS with many concurrent builds. |
| **Run custom logic safely** | Isolation for untrusted logic | **WASM (wasmtime) or Rhai — in-process, identical offline & online.** |

**Critical correction:** you **cannot compile native Rust inside a WASM sandbox**. WASM is for *running* code safely, not *building* Rust. "WASM sandbox to build code" is not viable; "WASM/Rhai sandbox to run per-type hooks" is the right use and unifies offline + online because the runtime embeds directly in both binaries.

---

## 4. Layer 1 — Dynamic CRUD engine (the runtime spine)

Unchanged from [01](01-architecture.md)/[03](03-content-type-builder-logic.md): schema JSON → `dynamic-store` builds SeaQuery DML/DDL at runtime; the Axum router is hot-rebuilt on CTB Save so new types serve immediately. This is the default execution path for **every** content-type, in both modes.

- Handles ~95% of all functionality (pure CRUD) with zero codegen and zero compile.
- Instant new-type latency, no restart, trivially offline.
- One binary serves unlimited content-types (multi-tenant friendly).

**Content Manager as gateway:** the gateway routes by content-type `uid` into this **one** generic engine, optionally intercepted by L2 hooks. It does **not** route to per-type compiled binaries. (If a type is later *ejected* — L3 — the gateway may proxy to that standalone service; advanced, `[LATER]`.)

---

## 5. Layer 2 — Scriptable hooks & policies (where a sandbox earns its place)

Reproduce Strapi's extensibility (lifecycle hooks, policies, custom validators) **without recompiling Rust**.

### 5.1 Hook points (mirror Strapi's document lifecycle)
`beforeCreate`, `afterCreate`, `beforeUpdate`, `afterUpdate`, `beforeDelete`, `afterDelete`, `beforeFindMany`, `afterFindMany`, plus request-level **policies** (allow/deny) and per-field **validators**.

Each hook receives a JSON context (`{ event, uid, action, data, where, user, locale, state }`) and may return a modified `data`/`where`, throw a validation error, or short-circuit (policy deny). The engine invokes registered hooks around every `dynamic-store` operation.

### 5.2 Engine choice
- **Phase-1 default: Rhai** — embeds in one crate, no external toolchain, no build step, Rust-flavored syntax, easy to sandbox (disable file/net, set op/time limits). Ideal for offline.
- **Phase-2 upgrade: WASM components (wasmtime + the component model)** — language-agnostic (author hooks in any language that targets WASM), stronger isolation, resource limits/fuel metering. Precompiled to a `.wasm` **once**, then hot-loaded into both binaries. Never compiled on the user's device.

Both run **in-process** and **identically** offline and online. Scripts are stored per-type in a system table and sync like any other row (see §8).

### 5.3 Storage
New system table:

**`extension_script`**
`id, schema_uid (FK), kind (hook|policy|validator), hook_point (nullable), engine (rhai|wasm), source_or_bytes, enabled BOOL, version, created_at, updated_at` + the sync columns from [07 §3](07-offline-sync.md) (`document_id`, `sync_version`, `origin_node_id`, `deleted_at`).

---

## 6. Layer 3 — Eject / export codegen (Baker + MiniJinja)

For users who want to **own the code**, deploy a **standalone** service with no `ferriscms` runtime dependency, or need **maximum compiled performance / compile-time type safety**.

### 6.1 What Baker gives us
Baker is a Rust scaffolding CLI whose engine is **MiniJinja**. Relevant features:
- `.baker.j2` templates and **loop templates** (one file per item — e.g. one entity per content-type).
- Codegen-shaped built-in filters: `snake_case`, `pascal_case`, `plural`, `singular`, `foreign_key`, `table_case`.
- `baker generate` to scaffold and **`baker update`** to re-render when the schema changes, emitting **git-style conflict markers** and preserving hand edits + stored answers (`.baker-generated.yaml`).
- Language-agnostic pre/post hooks (separate processes).

> Baker is a *whole-project* generator built on MiniJinja; do not put it on the request hot path. Use it (or MiniJinja directly) only for the eject feature.

### 6.2 The eject pipeline
```
schema JSON  ──►  answers/context  ──►  Baker template set  ──►  generated Rust project
   │                                          (MiniJinja)               │
   │                                                                    ▼
   │                                                          commit to git (schema + code)
   │                                                                    │
   ├── online:  Docker build container ── cargo build ── artifact/deploy (opt CI/CD)
   └── offline: write project to disk ── user runs `cargo build` themselves
```

Generated project = a real Axum + SeaORM 2.0 service: dense-format entities per content-type, typed columns, routers/handlers per type, migrations. The template context is derived from the same `Schema` model in [03 §3](03-content-type-builder-logic.md), so the eject output and the dynamic engine agree on shape.

### 6.3 Template set (initial)
`entity/<type>.rs.baker.j2` (loop over content-types), `router/<type>.rs.baker.j2`, `handlers/<type>.rs.baker.j2`, `migration/mXXXX_<type>.rs.baker.j2`, plus fixed `main.rs`, `Cargo.toml`, `config`. Relations/components/DZ map to the storage rules in [02 §3](02-data-model.md).

### 6.4 Re-eject on schema change
Store answers per project; on a later schema change, run `baker update` → conflict markers where the user hand-edited generated files. This is the one place the "code as a versioned artifact" idea lives.

### 6.5 Build isolation
- **Online:** a single resource-limited **Docker** build container (or a small build queue). **Kubernetes only** if `ferriscms` is run as a multi-tenant SaaS compiling many tenants concurrently — otherwise it is over-engineering.
- **Offline:** **do not** embed a toolchain. Emit the project and let the user build it. (Optional future: detect a local toolchain and shell out, `[LATER]`.)

---

## 7. Git strategy

- **Always** git-trackable: the **schema JSON** for every content-type/component (this is exactly what Strapi commits under `src/api/**/schema.json`). Gives versioning, audit, PR review of structural changes, and GitOps deploy with near-zero cost.
- **Only in eject mode:** git-track the generated Rust project.
- Git commits work **offline** (local repo); pushing to a remote rides the existing online/sync path ([07](07-offline-sync.md)). No new network machinery required for local history.
- Ties into `schema_change_log` ([02 §7](02-data-model.md)): each CTB Save can optionally produce a commit.

---

## 8. Offline / online parity & sync

- L1 and L2 run in-process in **both** binaries; behavior is identical by construction (same `services` + `dynamic-store` + script engine).
- `extension_script` rows (L2) carry sync columns and replicate via the sync engine ([07 §3–§5](07-offline-sync.md)) with the same LWW rules as content; script changes are structural-ish but low-risk (no DDL), so row-level LWW is acceptable.
- L3 artifacts are **not** synced as runtime state — they are exported projects living in git, outside the sync domain.

---

## 9. Consequences

**Positive**
- Offline stays instant; no toolchain on user devices.
- Matches Strapi's actual (interpreted) runtime, so the clone is faithful.
- One runtime code path (L1) → far smaller bug surface than maintaining dynamic + generated paths in parallel.
- Sandbox is used where it genuinely helps (running untrusted logic), not for the impossible/expensive case (compiling Rust in WASM, per-type builds).
- Codegen superpower is available (L3) for those who want ejectable, compiled, ownable code.

**Negative / risks**
- Two *logic* authoring surfaces long-term (Rhai now, WASM later) — mitigate by keeping the hook context contract identical across engines.
- Eject output can drift from dynamic behavior if templates lag the engine — mitigate by generating from the same `Schema` model and snapshot-testing eject output.
- Docker build latency + resource use in online eject — mitigate with a queue + caching (`sccache`, warm base image).
- Script sandbox is a security boundary — enforce op/time/memory limits (Rhai) or fuel/epoch limits (wasmtime); no ambient file/net access.

---

## 10. Rejected alternatives

- **Compile per content-type at runtime and load a dylib/service.** Rejected: build latency, restart/hot-load complexity, breaks offline, and unnecessary for Strapi parity.
- **WASM sandbox as the *build* environment.** Rejected: cannot compile native Rust inside WASM.
- **Kubernetes for build orchestration by default.** Rejected as over-engineering unless multi-tenant SaaS scale is an explicit goal.
- **Codegen as the primary runtime (Strapi-style file generation, but compiled).** Rejected: the compiled twist removes the very property (interpret-and-go) that makes Strapi's approach viable.

---

## 11. Phasing (folds into [08-roadmap.md](08-roadmap.md))

- **Phase 3:** L1 dynamic engine (already planned) is the sole execution path.
- **Phase 4:** L2 hooks/policies via **Rhai** + `extension_script` table + sync.
- **Phase 5:** L3 eject pipeline (Baker templates + git + Docker build) **and/or** L2 upgrade to **wasmtime** WASM components — prioritize per demand.
- **`[LATER]`:** gateway proxy to ejected standalone services; local-toolchain offline builds; Kubernetes build fan-out for multi-tenant SaaS.

---

## 12. Open decisions (to confirm)

1. **Scripting engine order** — start Rhai (recommended) then add WASM? Or go straight to wasmtime?
2. **Is "eject to standalone project" a product goal for v1**, or a later differentiator?
3. **Multi-tenant SaaS?** Determines whether K8s build fan-out is ever needed.
4. **Auto-commit schema JSON on every CTB Save**, or only on explicit "Export/Commit"?
