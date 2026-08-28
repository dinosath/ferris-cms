Ferris Application Platform — Implementation Plan
1. Vision
Transform Ferris CMS from a traditional headless CMS into a declarative application platform combining the strongest ideas from Strapi and NocoDB while solving their recurring complaints around lock-in, migrations, performance, permissions, extensibility, workflows, and production deployment.
Core promise
Build it visually. Run it as Ferris. Export it as a real application.
Ferris should let users build:
CMS/content applications
Database/business-data applications
Internal tools
CRUD applications
Customer portals
Workflow-driven applications
API backends
Admin panels
Dashboards
Hybrid applications combining local and external data
The platform must not make Ferris Runtime a permanent dependency. The Ferris Application Model is the source of truth and can be compiled into a standalone production application.
2. Product Architecture
Ferris should consist of four major layers:
Ferris Studio
                              |
                    Ferris Application Model
                              |
          +-------------------+-------------------+
          |                   |                   |
      Datasources          Resources          Workflows
          |                   |                   |
          +-------------------+-------------------+
                              |
                  Interfaces + Policies
                              |
                  +-----------+-----------+
                  |                       |
             Ferris Runtime           Compiler
                  |                       |
                  |                 +-----+------+
                  |                 |            |
                  |               Rust       Future targets
                  |               Axum
                  |               PostgreSQL
                  |               Kubernetes
                  |
             PostgreSQL / external systems
2.1 Ferris Studio
Visual application builder.
Responsibilities:
Resource/content type builder
Datasource management
Interface builder
Workflow builder
Permission/policy editor
Integration management
Application settings
Preview
Deployment
Import/export
Code generation
2.2 Ferris Application Model
The central declarative model.
It must describe:
Applications
Datasources
Resources
Fields
Relations
Constraints
Indexes
Interfaces
Views
Workflows
Policies
Authentication
Integrations
Jobs
Environment configuration
Deployment requirements
The model must be versioned and Git-friendly.
2.3 Ferris Runtime
Optional runtime for users who do not want generated source code.
Responsibilities:
API
Authentication
Authorization
Admin/application UI
Datasource adapters
Workflow execution
Background jobs
Audit logging
Observability
2.4 Ferris Compiler
Compile the application model into deployable applications.
Initial target:
Rust
Axum
Tokio
PostgreSQL
SQLx and/or SeaORM
OpenTelemetry
tracing
metrics
Docker
Kubernetes
Helm
Generated applications must be standalone.
3. Guiding Principles
3.1 No vendor lock-in
Everything created in Ferris must have a portable representation.
Users must be able to:
Export project
Export schema
Export workflows
Export interfaces
Export permissions
Export migrations
Generate source code
Generated applications must continue working without Ferris.
3.2 Model first, UI second
The visual UI must manipulate the Application Model rather than storing application logic exclusively inside UI metadata.
3.3 Database transparency
Ferris must expose database concepts rather than hiding them.
Users should be able to control:
Primary keys
Foreign keys
Unique constraints
Check constraints
Indexes
Composite indexes
Relations
Transactions
Migrations
3.4 Progressive complexity
Simple users should see:
Table
Fields
Views
Forms
Workflows
Advanced users should be able to access:
SQL
Indexes
Query plans
Policies
Generated code
Custom Rust
3.5 One security model
Authorization must be consistent across:
UI
API
workflows
generated applications
external integrations
Support:
RBAC
ABAC/policies
row-level permissions
field-level permissions
view permissions
workflow permissions
3.6 Production-grade output
Generated applications should not be demo CRUD applications.
They should include:
structured logging
tracing
OpenTelemetry
metrics
health checks
readiness/liveness
graceful shutdown
error handling
database pooling
timeouts
rate limiting
security headers
migrations
containerization
Kubernetes deployment
4. Phase 0 — Architecture Foundation
Goals
Establish the internal architecture before implementing individual features.
Tasks
Audit existing Ferris CMS architecture.
Identify current content-type model.
Identify current database schema representation.
Identify current workflow representation.
Identify current permissions model.
Identify current admin UI architecture.
Identify reusable existing components.
Separate platform model from presentation/UI state.
Define Application Model specification.
Define model versioning strategy.
Define migration strategy.
Define extension API.
Define compiler interfaces.
Deliverables
docs/
├── architecture.md
├── application-model.md
├── datasource-model.md
├── workflow-model.md
├── interface-model.md
├── authorization-model.md
├── code-generation.md
└── compatibility.md
5. Phase 1 — Application Model
Build the central declarative model.
5.1 Application
application:
  name: crm
  version: 1
Support:
name
description
version
environment configuration
dependencies
modules
5.2 Resource
Unify the concepts of:
Strapi Content Type
NocoDB Table
External Resource
Example:
resource:
  name: Customer
  source: postgres
  table: customers

  capabilities:
    create: true
    read: true
    update: true
    delete: true
5.3 Resource capabilities
Support:
CRUD
bulk operations
transactions
subscriptions
search
filtering
aggregation
synchronization
draft/publish
versioning
localization
audit
5.4 Fields
Support:
text
long text
rich text
number
decimal
boolean
date
datetime
time
JSON
UUID
enum
email
URL
media
relation
computed
formula
Each field should support:
validation
default value
required
visibility
editability
indexing
permissions
conditional rules
5.5 Relations
Support:
one-to-one
one-to-many
many-to-one
many-to-many
self-reference
polymorphic relations where practical
6. Phase 2 — Datasource System
This is one of the most important Ferris differentiators.
6.1 Datasource abstraction
Create a stable datasource interface.
Initial adapters:
PostgreSQL
MySQL
SQLite
REST
GraphQL
CSV
JSON
Future:
MSSQL
MongoDB
S3
Kafka
GitHub
ERP systems
CRM systems
6.2 Resource introspection
Datasource adapters should expose:
schemas
tables
columns
types
primary keys
foreign keys
indexes
constraints
relations
Allow:
Connect datasource
       ↓
Inspect schema
       ↓
Select resources
       ↓
Generate Ferris resources
6.3 Resource modes
Support:
Native
Ferris owns the schema.
External
Ferris consumes an existing resource.
Read-only
Ferris can query but not mutate.
Read/write
Ferris can mutate.
Synced
Ferris maintains a local representation.
Virtual
Resource is backed by an API/query rather than a physical table.
7. Phase 3 — Database and Migration Engine
Solve one of the biggest recurring Strapi complaints.
Requirements
Declarative schema diff
Migration generation
Migration preview
Migration execution
Migration verification
Rollback where safely possible
Seed data
Data migrations
Index management
Constraint management
CLI:
ferris migrate plan
ferris migrate generate
ferris migrate apply
ferris migrate verify
Never silently modify production schemas.
Provide:
Schema change detected

ADD:
  index invoices_customer_id_idx

ALTER:
  invoices.status

DROP:
  legacy_column

Migration:
  0007_add_invoice_indexes.sql
8. Phase 4 — NocoDB-style Interfaces
Interfaces become first-class application artifacts.
Required interfaces
Grid
Form
Detail
Kanban
Calendar
Gallery
Timeline
Dashboard
Master/detail
Custom page
Grid
Support:
sorting
filtering
grouping
pagination
column configuration
inline editing
bulk editing
bulk actions
relation expansion
saved views
formulas
aggregations
Forms
Support:
validation
conditional fields
defaults
dynamic options
relation selectors
file upload
workflow submission
Interface permissions
A user may see:
Grid: yes
Form: yes
Delete: no
Cost field: hidden
Approval action: no
9. Phase 5 — Query Engine
Address recurring NocoDB performance complaints.
Do not implement relations using uncontrolled N+1 requests.
Query abstraction
Create a query representation:
Query
├── fields
├── filters
├── joins
├── sort
├── pagination
├── aggregation
└── permissions
Compile this into optimized datasource-specific queries.
Query inspector
Expose:
generated query
execution time
rows scanned
rows returned
indexes used
warnings
Example:
Query: Orders Grid

Execution:
  18ms

Rows:
  scanned: 1,204
  returned: 50

Indexes:
  orders_customer_id_idx

Warnings:
  none
Performance protections
DataLoader/batching
query caching
pagination
virtualized grids
lazy relation loading
query complexity limits
timeout controls
10. Phase 6 — CMS Capabilities
Implement the Strapi strengths.
Content features
Draft/publish
Content versions
Revision history
Localization
Media library
Components
Repeatable components
Dynamic structures
Hierarchical content
Scheduled publishing
Preview
Editorial workflow
Content state
Use explicit state machines where appropriate:
draft
  ↓
review
  ↓
approved
  ↓
published
  ↓
archived
Avoid hard-coding CMS-specific lifecycle behavior into the database layer.
11. Phase 7 — Workflow Engine
Create a NocoDB/n8n-inspired visual workflow system.
Nodes
Triggers
record created
record updated
record deleted
schedule
webhook
API call
manual
external event
Logic
condition
switch
loop
parallel
merge
delay
retry
timeout
Actions
database
HTTP
email
webhook
file
notification
script
approval
AI
custom Rust node
Workflow execution
Every workflow execution should have:
execution ID
status
input
output
logs
timing
retries
errors
audit trail
Example:
Order created
      ↓
Validate
      ↓
Inventory check
      ↓
Amount > €5,000?
    /       \
  yes       no
   ↓         ↓
Approval   Continue
   ↓         ↓
      Create invoice
             ↓
          Send email
Reliability
Implement:
retries
idempotency
timeouts
dead-letter handling
durable execution
compensation
concurrency limits
The architecture should permit a future Temporal-like execution backend without changing workflow definitions.
12. Phase 8 — Authorization System
Create a unified authorization engine.
Roles
Admin
Manager
Sales
Finance
Customer
Policies
Support:
resource
field
record
interface
workflow
action
API
Example:
invoice.update

allow:
  user.company_id == invoice.company_id

deny:
  invoice.status == "paid"
Permission debugger
Provide:
Why can't this user edit Invoice #182?

DENIED

Action:
  invoice.update

Role:
  Sales

Record policy:
  company_id == user.company_id

Actual:
  17 != 23
This should be available in development/admin mode.
13. Phase 9 — Authentication
Support:
email/password
sessions
JWT
OAuth/OIDC
SSO
API keys
service accounts
machine-to-machine authentication
Separate:
Authentication
from:
Authorization
so generated applications can replace the authentication provider without changing the application model.
14. Phase 10 — Integrations
Create an integration framework.
Integrations should expose capabilities rather than custom UI-only implementations.
Example:
Stripe
├── customers.read
├── customers.create
├── invoices.read
└── payments.create
Potential integrations:
Stripe
email providers
Slack
GitHub
S3
webhooks
REST APIs
GraphQL
ERP
CRM
Integrations should be usable from:
workflows
interfaces
backend code
generated applications
15. Phase 11 — AI Layer
AI should operate against the Application Model and authorization system.
AI capabilities
generate resource
generate fields
generate relationships
generate interfaces
generate workflows
explain permissions
explain schema
generate queries
diagnose slow queries
generate migrations
generate Rust code
generate tests
Example:
"Create a CRM for sales representatives."

AI generates:

Customers
Contacts
Deals
Activities
Users

Interfaces:
  Customer Grid
  Customer Detail
  Deal Kanban

Workflow:
  Deal Won → Create Invoice
Security
AI must never bypass Ferris permissions.
Every AI action must be evaluated against:
user
+
role
+
resource
+
action
+
policy
The AI must not receive inaccessible records merely because it is an AI assistant.
16. Phase 12 — Application Export
This is a core product feature, not an afterthought.
Project format
Use a Git-friendly structure:
my-app/
├── ferris.toml
├── resources/
├── datasources/
├── interfaces/
├── workflows/
├── policies/
├── integrations/
├── migrations/
├── seeds/
└── codegen/
Everything should be text-based where practical.
Commands
ferris export
ferris import
ferris validate
ferris diff
Support Git workflows:
Developer A
     ↓
Git commit
     ↓
CI
     ↓
Ferris validation
     ↓
Migration
     ↓
Deployment
17. Phase 13 — Rust Code Generator
Initial target:
Rust
Axum
Tokio
PostgreSQL
SQLx/SeaORM
Generated project
my-app/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── api/
│   ├── domain/
│   ├── repository/
│   ├── services/
│   ├── workflows/
│   ├── auth/
│   ├── generated/
│   └── extensions/
├── migrations/
├── tests/
├── Dockerfile
├── compose.yaml
├── helm/
└── README.md
Generated code principles
Generated code must be:
idiomatic
formatted
tested
documented
modular
independently buildable
Do not generate a monolithic source file.
18. Phase 14 — Generated Application Observability
Every generated service should include:
Logging
Use:
tracing
tracing-subscriber
OpenTelemetry
Support:
traces
metrics
logs
Metrics
Expose:
HTTP request count
HTTP latency
HTTP errors
database latency
database pool utilization
workflow executions
workflow failures
queue depth
application-specific metrics
Health
Provide:
/health
/ready
with Kubernetes-compatible semantics.
19. Phase 15 — Kubernetes Generator
Generate production-oriented Kubernetes deployment.
Output
helm/
└── my-app/
    ├── Chart.yaml
    ├── values.yaml
    └── templates/
        ├── deployment.yaml
        ├── service.yaml
        ├── ingress.yaml
        ├── configmap.yaml
        ├── secret.yaml
        ├── hpa.yaml
        ├── pdb.yaml
        ├── servicemonitor.yaml
        └── networkpolicy.yaml
Support:
Deployment
Service
Ingress
ConfigMap
Secret references
HPA
PDB
NetworkPolicy
ServiceMonitor
resource requests/limits
probes
graceful shutdown
Provide sensible defaults but allow customization.
20. Phase 16 — Deployment Targets
Initial targets:
Docker Compose
Docker
Kubernetes
Helm
Future:
Kubernetes + ArgoCD
AWS
GCP
Azure
Hetzner
bare metal
Generated infrastructure must remain editable.
21. Phase 17 — Custom Code / Escape Hatch
Never force users to wait for Ferris to implement every feature.
Support:
Custom API endpoints
GET /custom/report
Custom Rust services
src/extensions/reporting.rs
Custom workflow nodes
CustomPaymentNode
Custom fields
MoneyField
Custom datasource adapters
EpsilonSmartDatasource
Custom interfaces
Allow application-specific UI components without modifying generated internals.
22. Phase 18 — Safe Code Generation
Generated code must be separated from user code.
src/
├── generated/
└── app/
Regeneration replaces only:
src/generated/
User code in:
src/app/
src/extensions/
must remain intact.
Provide extension points so generated code calls user-defined implementations where appropriate.
23. Phase 19 — Reverse Engineering / Import Existing Applications
Eventually support:
Existing PostgreSQL
        ↓
Datasource introspection
        ↓
Ferris Application Model
and:
Existing API
        ↓
OpenAPI
        ↓
Ferris Datasource
Future:
Existing Rust application
        ↓
Ferris introspection
        ↓
Application Model
This enables Ferris to be adopted incrementally.
24. Phase 20 — Testing
Testing must exist at every layer.
Application Model
schema validation
compatibility tests
migration tests
Datasources
PostgreSQL integration tests
MySQL integration tests
API adapter tests
Interfaces
component tests
accessibility tests
interaction tests
Workflows
deterministic workflow tests
retry tests
failure tests
concurrency tests
Authorization
Test:
role × action × resource × record × field
Generated applications
Every generated application must compile and pass generated tests.
25. Phase 21 — End-to-End Test Suite
Use Rust-based testing where practical.
Test complete scenarios:
Create application
  ↓
Create resource
  ↓
Create interface
  ↓
Create workflow
  ↓
Create permissions
  ↓
Generate application
  ↓
Build Docker image
  ↓
Run PostgreSQL
  ↓
Run application
  ↓
Execute API tests
  ↓
Execute workflow
  ↓
Verify telemetry
Include screenshot-based UI regression tests for Studio.
26. Phase 22 — Observability for Ferris Itself
Ferris SaaS must observe:
API latency
database latency
datasource latency
workflow latency
code generation duration
deployment duration
failed migrations
failed workflows
queue depth
active users
resource counts
Use the same observability principles that generated applications receive.
27. Phase 23 — SaaS Architecture
Separate the SaaS control plane from customer applications.
Ferris Cloud
                         |
                 Control Plane
                         |
       +-----------------+-----------------+
       |                 |                 |
    Projects          Billing          Identity
       |
   Application Model
       |
       +------------------+
       |                  |
  Ferris Runtime     Code Generation
       |                  |
       ↓                  ↓
 Customer App       Customer Repository
Customers should be able to:
use Ferris-hosted runtime
deploy generated application themselves
export everything
connect their own databases
connect external APIs
28. Multi-tenancy
Design tenant isolation from the beginning.
Support multiple strategies:
Shared database
tenant_id
Separate schema
tenant_a.*
tenant_b.*
Separate database
tenant A DB
tenant B DB
The Application Model should not assume one strategy.
29. Versioning
Version all major model components.
Application v1
Resource v4
Workflow v7
Interface v3
Policy v2
Support:
ferris diff
Example:
Resource Invoice

+ field: payment_reference
~ field: status
+ index: invoice_customer_status_idx
This should integrate naturally with Git.
30. Marketplace / Extensions
Create a stable extension model.
Possible extension types:
Datasource
Field
Interface
Workflow node
Authentication provider
Integration
Code generator
Deployment target
Theme
AI provider
Extensions must declare compatibility:
requires:
  ferris: ">=1.4 <2"
Avoid plugins modifying internal implementation details.
31. Developer Experience
Provide:
ferris init
ferris dev
ferris validate
ferris diff
ferris migrate
ferris generate
ferris build
ferris test
ferris export
ferris deploy
A developer should be able to run:
ferris dev
and receive:
Studio
API
Workflow worker
PostgreSQL
Observability
with one command.
32. CLI Code Generation
Example:
ferris generate --target rust-axum
Options:
ferris generate \
  --target rust-axum \
  --database postgres \
  --observability opentelemetry \
  --deployment kubernetes
The generator should be deterministic.
The same Application Model should produce the same generated output.
33. Generated Application Configuration
Generate environment-based configuration.
Example:
DATABASE_URL
OTEL_EXPORTER_OTLP_ENDPOINT
RUST_LOG
HTTP_PORT
AUTH_ISSUER
AUTH_AUDIENCE
Secrets must never be committed into generated source.
34. Migration from Existing Ferris CMS
Do not rewrite Ferris from scratch.
Perform incremental migration:
Step 1
Map existing Content Types → Resources.
Step 2
Map existing fields → Resource Fields.
Step 3
Map existing relations → Relations.
Step 4
Map existing workflows → Workflow Model.
Step 5
Map existing permissions → Policy Model.
Step 6
Replace existing admin screens with I
