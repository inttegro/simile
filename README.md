# Simile

Simile lets a service provider distribute a faithful, locally runnable model of its system. It
emulates the service's API, state transitions, and asynchronous behaviour so developers can build
and test complete integrations without depending on hosted test infrastructure.

An HTTP response is only the visible edge of many services. A request may create durable state,
enqueue work, emit an event, deliver a webhook, retry, or complete later. Simile treats those
effects as part of the operation rather than reducing the service to static request and response
fixtures.

The provider owns the behaviour definition. The developer runs it on their own machine or in CI:

```text
application under test
        |
        v
local Simile runtime ----> immediate response
        |
        +---------------> state changes
        +---------------> events, webhooks, retries, and delayed outcomes
```

This moves the execution and availability of test mode to the developer while leaving the service
provider in control of its semantics. Definitions can represent services such as Inttegro or
Stripe without requiring those platforms to operate a separate hosted environment for every
integration.

Read the [service charter](CHARTER.md) for the product boundary and design principles, and the
[compiler architecture](docs/architecture.md) for the source and ownership boundaries.

## Status

Simile is at its compiler-foundation stage. It can normalize OpenAPI 3.0, 3.1 and 3.2 contracts,
parse behavior modules written in HCL, compile both into one validated service model, and emit a
versioned `.similec` runtime artifact. Request and response schemas are compiled into the artifact.
The runtime can validate decoded request values, synthesize varied contract-sound requests, and
synthesize sound JSON responses either from the contract alone or with compatible values projected
from a validated request. Provider-authored `x-simile-constraints` can prove and construct field
ordering, conditional implications, equality across array items, numeric equations and aggregates,
derived string templates, and finite lifecycle snapshots. A protocol-neutral runtime coordinates request validation, response
synthesis, and the declared asynchronous work for one operation. HTTP serving, state, and
asynchronous resource execution remain subsequent runtime layers; the current CLI does not yet
serve the compiled service. It can execute one operation directly from a local or hosted OpenAPI
contract.

The available foundation commands are:

```console
$ simile version
$ simile doctor
$ simile --output json doctor
$ simile compile openapi.yaml --behavior orders.hcl --artifact service.similec
$ simile inspect service.similec
$ simile run openapi.yaml --operation-id createOrder
$ simile run --spec-url https://example.com/openapi.yaml --operation "GET /health"
```

`run` accepts exactly one contract source and one operation selector. A positional source path is
mutually exclusive with `--spec-url`; a structural `--operation "METHOD /path"` selector is
mutually exclusive with the exact, case-sensitive OpenAPI `--operation-id` token. Hosted contracts
are read into memory and are not saved to disk.

When an operation declares a request body and `--body` is absent, Simile synthesizes a fresh valid
value from the contract and validates it before execution—even when OpenAPI
marks that body optional. A provided `--body` is a normalized JSON value that Simile validates
against the selected media type. Only operations with no request-body declaration run without one.
The command prints the validated request, synthesized response, and any declared asynchronous
work when `--verbose` is passed. Without `--verbose`, stdout contains only the response payload as
pretty-printed JSON, making `simile run` directly composable with tools such as `jq`. A bodyless
response is printed as `null`. The command does not execute asynchronous work or start an HTTP
server.

OpenAPI JSON/YAML and HCL are source formats. They are not the runtime storage format. A
`.similec` file contains a compact indexed model encoded as deterministic CBOR, wrapped in a
versioned envelope with a payload length and SHA-256 integrity digest. The string table,
operation records, value arena, async-operation references, and resource references are all
ordered before encoding, so identical compiler input produces identical artifact bytes. See the
[compiled artifact format](docs/compiled-artifact.md) for the compatibility contract.
The supported request-validation boundary is documented in
[request validation](docs/request-validation.md). Response selection and synthesis are documented
in [response synthesis](docs/response-synthesis.md). The transport-independent execution boundary
is documented in [operation execution](docs/operation-execution.md). The shared proof boundary for
OpenAPI rules and cross-field relationships is documented in
[payload soundness](docs/payload-soundness.md).

OpenAPI is sufficient on its own. For operations without an HCL overlay, Simile derives its
default behavior from the OpenAPI operation. Fresh generated values vary within the schema; a
long-lived runtime replays an outcome only when its adapter supplies the same idempotency key or
GET resource identity. HCL is used only when a provider needs
to select a declared response or describe observable effects that OpenAPI cannot express:

```hcl
resource "db" "orders" {}
resource "event_bus" "commerce" {}

async "publish_order" {
  uses = ["db.orders", "event_bus.commerce"]
}

behavior "createOrder" {
  response { status = 201 }
  trigger "publish_order" {}
}
```

Resource kinds are intentionally abstract. A definition may declare `db`, `cache`, `queue`,
`mail`, `object_store`, `event_bus`, `scheduler`, or `http`; it cannot couple local emulation to a
production vendor or reveal private implementation checks.

## Architecture

Simile follows a pipeline of narrow ownership boundaries:

- `simile-cli` owns argument parsing, terminal rendering, logging, and process exit behaviour.
- `simile-core` owns application context, domain types, and operations.
- `simile-artifact` lowers the compiled service into the indexed runtime model and owns the
  versioned `.similec` envelope.
- `simile-openapi` converts supported OpenAPI feature lines into a canonical HTTP contract.
- `simile-schema` compiles request and response schemas into portable graphs, validates decoded
  values and typed cross-field relationships, and performs seeded contract-sound synthesis.
- `simile-hcl` parses and validates optional behavior modules against that contract.
- `simile-compiler` coordinates both sources and produces a compiled service.
- `simile-runtime` loads an operation from an artifact, validates normalized values independently
  of their wire encoding, synthesizes responses, replays identified invocations, plans declared
  asynchronous work, and reports protocol or schema failures without depending on source formats
  or a server framework.
- `simile-spec` contains only the version-neutral model shared by compilation units.
- Files under `commands/` are thin adapters. Behaviour belongs under `ops/`.
- Human output goes to stdout, diagnostics go to stderr, and JSON never contains ANSI colour.

Commands must not accumulate application logic. Add a command by declaring its arguments in
`simile-cli`, converting them into a core request, and delegating to a matching core operation.

## Development

The repository pins its minimum supported Rust toolchain. Common checks are available as Cargo
aliases:

```console
$ cargo test-all
$ cargo lint
$ cargo fmt --all --check
```

If `cargo-nextest` is installed locally, the checked-in profile can be used with:

```console
$ cargo nextest run --workspace --all-features
```

Release archives and installers are configured through `dist-workspace.toml`.
