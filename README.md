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

## Install

Install the latest release on macOS or Linux with the shell installer:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/inttegro/simile/HEAD/install.sh)"
```

The bootstrap downloads the platform-specific release artifact, verifies its checksum, and installs
`simile` under Cargo's binary directory. Set `SIMILE_VERSION` to install a specific release:

```bash
SIMILE_VERSION=0.2.0 /bin/bash -c \
  "$(curl -fsSL https://raw.githubusercontent.com/inttegro/simile/HEAD/install.sh)"
```

With Homebrew:

```bash
brew tap inttegro/tap
brew trust --formula inttegro/tap/simile
brew install inttegro/tap/simile
```

## Status

Simile is at its compiler-foundation stage. It can normalize OpenAPI 3.0, 3.1 and 3.2 contracts,
parse behavior modules written in HCL, compile both into one validated service model, and emit a
versioned `.similec` runtime artifact. Request and response schemas are compiled into the artifact.
The runtime can validate decoded request values, synthesize varied contract-sound requests, and
synthesize sound JSON responses either from the contract alone or with compatible values projected
from a validated request. Provider-authored `x-simile-constraints` can prove and construct field
ordering, conditional implications, equality across array items, numeric equations and aggregates,
relative bounds, distinct selectors, scalar set relationships, derived string templates, and finite
lifecycle snapshots. Constraints declared on array item schemas apply independently to every item.
For contracts without those extensions, a separately compiled provider-neutral heuristic plan can
repair a small, curated set of obvious generation inconsistencies. Heuristics improve generated
examples only; they never become request-validation rules or proof obligations.
A protocol-neutral runtime coordinates request validation, response synthesis, and the declared
asynchronous work for one operation. A separate HTTP adapter can now compile a runnable service
configuration, listen on a local port, match its OpenAPI routes, validate JSON request bodies, and
return generated responses. OpenAPI examples, reusable provider-authored outcome sets, public
request predicates, opaque scenarios, and service-wide protocol-error mappings now compile into a
named outcome catalog with fidelity provenance. Durable resource state and asynchronous resource
execution remain subsequent runtime layers; declared async work is planned but is not yet
performed.

The available foundation commands are:

```console
$ simile version
$ simile doctor
$ simile --output json doctor
$ simile compile openapi.yaml --behavior orders.hcl --artifact service.similec
$ simile inspect service.similec
$ simile run openapi.yaml --operation-id createOrder
$ simile run openapi.yaml --operation-id createOrder --request request.json
$ cat request.json | simile run openapi.yaml --operation-id createOrder --request -
$ simile run --spec-url https://example.com/openapi.yaml --operation "GET /health"
$ simile run openapi.yaml --operation-id createOrder --heuristics conservative
$ simile run openapi.yaml --behavior payments.hcl --operation-id createPayment --outcome card_declined
$ simile run openapi.yaml --behavior payments.hcl --operation-id createPayment --scenario payment_declined
$ simile register inttegro --config inttegro.simile.hcl
$ simile register inttegro --config https://example.com/inttegro.simile.hcl
$ simile update inttegro
$ simile update inttegro --config https://example.com/inttegro.simile.hcl
$ simile remove inttegro --confirm inttegro
$ simile start inttegro
$ simile start inttegro --config inttegro.simile.hcl
$ simile start inttegro --config inttegro.simile.hcl --port 8484
$ simile start inttegro --scenario payment_declined
$ simile start inttegro --fault-profile rough_network
```

`run` accepts exactly one contract source and one operation selector. A positional source path is
mutually exclusive with `--spec-url`; a structural `--operation "METHOD /path"` selector is
mutually exclusive with the exact, case-sensitive OpenAPI `--operation-id` token. Hosted contracts
are read into memory and are not saved to disk.

When an operation declares a request body and `--request` is absent, Simile synthesizes a fresh
valid value from the contract and validates it before execution—even when OpenAPI marks that body
optional. `--request` accepts a complete request object as inline JSON, reads it from a bare file
path (the `@file` spelling remains supported), or reads standard input when passed `-`:

```json
{
  "headers": { "Content-Type": "application/json" },
  "body": { "product": "custom_jingle", "quantity": 4 }
}
```

Header names are matched case-insensitively. The `Content-Type` header selects the OpenAPI media
type for a provided body; `--content-type` remains available only when Simile is synthesizing the
request. A supplied request object can omit `body`; required-body operations reject that request,
while optional-body and bodyless operations execute without a body.
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
The deliberately weaker generation-only inference boundary is documented in
[generation heuristics](docs/generation-heuristics.md).
Named responses, protocol errors, scenarios, provenance, and the public export boundary are
documented in [outcome fidelity](docs/outcome-fidelity.md).

## Running a service

`simile register` reads one strict HCL service configuration from a local path or HTTP(S) URL,
compiles its OpenAPI contract and optional behavior modules, and atomically installs a validated
runtime snapshot under `$HOME/.simile`. The positional name must match the service label. The
registered artifact is loaded by `simile start NAME` without reopening local sources or refetching
a remote contract. Pass `--config` to `start` for an ad-hoc, unregistered run. `--port` overrides
either configuration for that invocation; the listener host remains configuration-owned.

`simile update NAME` recompiles the registered HCL snapshot after reloading its OpenAPI and
behavior sources. Pass `--config URL_OR_FILE` to replace the HCL configuration source at the same
time. Every successful update installs a new immutable revision and atomically advances the
service's `current` pointer; a failed load or compilation leaves the active revision unchanged.

`simile remove NAME --confirm NAME` permanently deletes that service's current pointer, immutable
revisions, and local state. The exact-name confirmation is required; other registered services and
the registry layout are preserved.

```hcl
service "inttegro" {
  listen {
    host = "127.0.0.1"
    port = 4242
  }

  openapi {
    url = "https://example.com/commerce.yml"
  }

  behaviors = ["./orders.hcl"]
}
```

Use exactly one of `path` or `url` inside `openapi`. Relative OpenAPI and behavior paths resolve
from a local service configuration's directory. A remotely loaded HCL configuration must use an
OpenAPI URL and may use inline behavior declarations; imported behavior paths have no remote base
and are rejected. The default host is `127.0.0.1`; a port from 1 through 65535 is required.
Registering an existing name requires `--force`. Set `SIMILE_HOME` to override the registry root
for CI or isolated development. The server prefers literal routes over parameter
templates, returns `404` or `405` before entering the runtime, limits bodies to 8 MiB, and preserves
process-local replay for `Idempotency-Key` and repeated GET identities. See
[service configuration](docs/service-configuration.md) for the complete current boundary.

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

Providers can publish exact failure families once, attach them to operations, and expose opaque
scenario controls without shipping private decision logic. `simile inspect` lists every compiled
outcome and marks operations that still depend entirely on schema synthesis. See
[outcome fidelity](docs/outcome-fidelity.md) for the HCL forms and trust model.

Environmental faults are modeled independently from domain errors. Deterministic scenarios can
delay a response or disconnect after commit, while bounded seeded profiles can apply those faults
stochastically without ever inventing a bad parameter or declined payment. The runtime completes
and stores the service outcome before returning a protocol-neutral transport plan to the server;
an idempotent retry after a lost response therefore receives the committed result normally. See
[fault injection](docs/fault-injection.md) for the fault vocabulary and activation controls.

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
- `simile-heuristics` compiles provider-neutral, evidence-bearing generation hints without owning
  validation or provider-specific profiles.
- `simile-hcl` parses and validates optional behavior modules against that contract.
- `simile-compiler` coordinates both sources and produces a compiled service.
- `simile-runtime` loads an operation from an artifact, validates normalized values independently
  of their wire encoding, synthesizes responses, replays identified invocations, plans declared
  asynchronous work, and reports protocol or schema failures without depending on source formats
  or a server framework.
- `simile-server` owns HTTP listener lifecycle, route-template matching, wire limits, replay
  identity extraction, and translation between HTTP responses and the protocol-neutral runtime.
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
