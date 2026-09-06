# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/ferriscms-v0.2.0) - 2026-09-06

### Added

- *(auth)* wire OIDC SSO into the UI and add mock-IdP E2E test
- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- *(ai)* expose content-type/workflow actions to the assistant, chat table + overlay
- *(ai)* persist and restore assistant chat history in the UI
- *(ai)* test provider connection and auto-discover models
- *(ai)* auto-create a default model per provider and add provider via modal
- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import)* show mapping table from input fields on target selection
- *(import)* show mapping table with data examples + remap on target change
- *(import-export)* export filter builder UI
- *(import-export)* configurable CSV delimiter + header row
- *(import-export)* export field selection + reliable field projection
- *(import-export)* persist mapping presets in the database + UI
- *(import-export)* Content Manager contextual actions + prefer-uid prefill
- *(import-export)* add Import & Export system
- *(ui)* table-first Content-Type Builder and Content Manager navigation
- *(app)* content-type-aware node config + workflow import/export UI
- *(app)* API / Integrations credential management screen
- *(workflow)* n8n-style visual workflow automation engine
- *(cm)* condition-based filters, column sorting, and edit-view delete
- conditional field visibility (Strapi conditional fields)
- *(ctb)* auto-populate API ID singular/plural from display name
- *(ui)* accessibility and responsive polish
- *(ui)* Strapi-aligned Content-Type Builder polish
- *(ui)* Strapi-aligned Content Manager polish
- *(ui)* sticky sidebar and breadcrumb top bar in admin shell
- *(ui)* class-driven design-system components with new primitives
- *(ui)* redirect unauthenticated users to the login screen

### Fixed

- *(ai)* restore the Confirm actions card after reloading a conversation
- *(ctb)* require explicit removal instead of absence-as-deletion
- *(ui)* make action dispatchers reactive so Continue actually runs
- *(app)* run async UI actions from use_effect, not event handlers
- *(app)* complete connection creation in the workflow editor
- *(auth)* run login/register async from an effect, not the event handler
- *(auth)* propagate JWT to the HTTP transport
- *(ui)* repair broken design-token CSS in theme.rs
- *(ctb)* always show add-field button and surface real save errors

### Other

- Replace custom workflow engine with Open Workflow Specification (OWS)
- *(import)* extract plan_mappings and add unit tests
- *(ctb)* use cruet for kebab-case and pluralization
- run cargo fmt across the workspace
- Add WASM panic hook to surface Rust panics in the browser console
- Add toast + confirm dialog system
- Add Unpublish control and fix publish documentId bug
- Add Internationalization (locales) settings UI
- Add API Tokens service, endpoints, and Settings UI
- Add component and dynamic-zone form widgets to entry edit view
- Add per-row edit/delete actions to Content Manager list
- Add component, dynamic-zone, media, and UID field configuration to CTB
- Add Relation field configuration to CTB field-config modal
- Add Configure-the-view modal to Content Manager list toolbar
- Add Discard changes control to entry edit view
- Implement Media Library: backend storage + upload route + UI
- Add Settings > Users management UI with invite modal
- Add Settings > Roles management UI with permission matrix
- Align content navigation, CTB, RBAC, and UI with Strapi
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
