# Open Workflow Specification (OWS) — direct replacement

FerrisCMS has fully replaced its legacy, custom workflow engine with the
**Open Workflow Specification (OWS)** — the CNCF **Open Workflow DSL**
(Serverless Workflow, `1.0.3`). The legacy custom workflow representation
(flat `nodes`/`connections` with ad-hoc trigger nodes), its persistence model,
its execution engine and its editor have been removed. There is **no
compatibility shim or migration layer**: OWS is the only supported workflow
model and runtime.

## Canonical model

The canonical workflow document is a wrapper around the official
[`serverless_workflow_core`](https://github.com/open-workflow-specification/sdk-rust)
`WorkflowDefinition`, stored as **JSON** (and importable/exportable as
**YAML**). It lives in `crates/workflow/src/model.rs` as `OwsDocument`:

```jsonc
{
  "id": 1, "active": false, "version": 3,
  "createdAt": "...", "updatedAt": "...",
  "definition": {
    "document": { "dsl": "1.0.3", "namespace": "default", "name": "...", "version": "1.0.0" },
    "schedule": { "on": { "one": { "with": { "type": "content.created", "contentType": "api::article.article" } } } },
    "do": [
      { "setGreeting": { "set": { "greeting": "Hello" } } },
      { "publish": { "call": "cms.publishContent", "with": { "contentType": "api::article.article", "documentId": "${ .documentId }" } } }
    ]
  }
}
```

## Runtime

Workflow **execution** is performed by the official
[`ows-runtime-rust`](https://github.com/dinosath/ows-runtime-rust) runtime
library (the `ows-runtime` engine over the OWS SDK). `services/workflow/engine.rs`
builds an `ows_runtime::Runtime`, registers the FerrisCMS functions (CMS/media/
HTTP/integration) as `FunctionInvoker`s (`services/workflow/runtime_fn.rs`),
compiles the definition with `register_definition`, and runs it. The runtime's
jq runtime-expression engine (`${...}`) evaluates task config, `switch`
conditions, `for` loops, `try`/`catch` error handling and retries. Per-task
execution order is captured for execution inspection. Parsing/validation of
YAML and JSON documents uses the OWS DSL tooling (`ows_runtime_dsl`).

## Concept mapping

| Legacy custom workflow | OWS (Open Workflow DSL) |
| --- | --- |
| `Workflow` | `OwsDocument` (SDK `WorkflowDefinition` + app metadata) |
| Node types / node definitions | Task types (`set`, `do`, `for`, `fork`, `switch`, `try`, `emit`, `listen`, `raise`, `run`, `call`, `wait`) + **functions** in `crate::model::function` |
| Node `parameters` | Task `with` arguments / `set` map |
| Ports & connections/edges | `then` flow directives (and `switch` cases) instead of visual edges |
| Trigger nodes | `schedule.on` event filters (CMS content/media/user events, `webhook`, `manual`, `schedule`) |
| Event routing | OWS events + `schedule.on` dispatch (`services/workflow/triggers.rs`) |
| Conditions / decision nodes | `switch` task with `when` conditions |
| Variables / env values | `definition.metadata` seeded into the runtime context (`$context`/`$input`) |
| Credentials / secret refs | OWS `use.secrets` + saved credentials; functions resolve via `credentialId` |
| Execution status / logs | `OwsExecutionStatus`, `OwsTaskRunStatus`, `OwsExecution`, `OwsTaskRun` |
| Scheduling / webhook / manual / content triggers | `schedule.on` events, `schedule.cron`, manual `execute`, webhook path matching |
| Error outputs / failure handling | `try`/`catch` tasks, `onError` semantics, `raise` |

## Direct replacement plan

1. **Domain model** (`crates/workflow`) — removed the legacy model; the SDK
   `WorkflowDefinition` is wrapped as `OwsDocument`. Function catalog in
   `node.rs`; task-order resolution + flow-directive handling in `graph.rs`;
   OWS validation in `validation.rs`.
2. **Persistence** — the `workflow` table's `definition_json` now stores the
   canonical OWS document. Execution/task-run rows persist OWS status records.
   JSON is canonical; import/export also supports **YAML**.
3. **Runtime** — workflow execution is delegated to the official
   `ows-runtime-rust` engine. `services/workflow/engine.rs` builds an
   `ows_runtime::Runtime`, registers the FerrisCMS functions as
   `FunctionInvoker`s (`services/workflow/runtime_fn.rs`), compiles each
   definition with `register_definition`, and runs it with the runtime's jq
   expression engine, retries, timeouts and error handling.
4. **Triggers** — `services/workflow/triggers.rs` dispatches CMS events,
   media/user events, webhooks and schedules to workflows declared via
   `schedule.on`.
5. **API** — `api-rest/src/workflow.rs` reads/writes `OwsDocument`, validates
   OWS, executes, and exports/imports JSON or YAML.
6. **UI** — `app/src/screens/workflow_editor.rs` is an OWS task editor (catalog,
   task configuration, execution inspection); `workflows.rs` and
   `executions.rs` use the OWS list/detail shapes.
7. **Tests** — `workflow` unit tests (serialization/validation/order), `services`
   engine tests (branching, loops, errors, persistence), and `api-rest`
   integration tests (CRUD, activation, execution, webhook, content trigger,
   JSON+YAML import/export).

## Verification

```sh
cargo test -p workflow -p services -p api-rest
```

All suites pass, covering serialization, execution semantics across workflow
categories, persistence round-trips, trigger dispatch, and API behavior.
