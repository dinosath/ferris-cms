# FerrisCMS Documentation

> A **1:1, offline-first clone of Strapi** built entirely in **Rust**.

This folder documents the FerrisCMS project. It contains both the original
planning/design documents (`00-*.md`, `design.md`) and a consolidated,
implementation-focused set that describes how the system is actually built
today.

## New implementation docs

| Document | Purpose |
|---|---|
| [`STACK.md`](STACK.md) | The technology stack at a glance: layers, crates, runtime modes, deployment. |
| [`TECHNOLOGIES.md`](TECHNOLOGIES.md) | Deep dive on each technology and why it was chosen. |
| [`FEATURES.md`](FEATURES.md) | Catalog of implemented features, screen by screen. |
| [`IMPLEMENTATION.md`](IMPLEMENTATION.md) | How it is built: workspace layout, request flows, schema lifecycle, data layer, auth/RBAC, workflows, AI, testing. |

## Original planning / design documents

| Document | Purpose |
|---|---|
| [`00-overview.md`](00-overview.md) | Project overview and goals. |
| [`01-architecture.md`](01-architecture.md) | High-level architecture. |
| [`02-data-model.md`](02-data-model.md) | Data model design. |
| [`03-content-type-builder-logic.md`](03-content-type-builder-logic.md) | Content-Type Builder design. |
| [`04-rest-api.md`](04-rest-api.md) | REST API design. |
| [`05-ui-design-system.md`](05-ui-design-system.md) | UI design system. |
| [`06-ui-screens.md`](06-ui-screens.md) | UI screen specifications. |
| [`07-offline-sync.md`](07-offline-sync.md) | Offline-first sync design. |
| [`08-roadmap.md`](08-roadmap.md) | Roadmap. |
| [`09-codegen-and-sandbox.md`](09-codegen-and-sandbox.md) | Code generation and sandboxing. |
| [`10-deployment-modes-and-gitops.md`](10-deployment-modes-and-gitops.md) | Deployment modes and GitOps. |
| [`11-strapi-parity-audit.md`](11-strapi-parity-audit.md) | Strapi parity audit. |
| [`design.md`](design.md) | The master design document. |
