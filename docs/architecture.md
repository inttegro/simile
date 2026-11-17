---
title: Architecture
description: The ownership boundaries that keep source languages, runtime behavior, and transport separate.
---

Simile is organized around ownership boundaries rather than command names. OpenAPI parsing, HCL
semantics, schema proof, artifact encoding, operation execution, HTTP transport, and terminal
presentation are separate because each has a different reason to change.

OpenAPI is mandatory and complete for the current HTTP body contract. HCL is optional and additive
for behavior. Both disappear behind a compiled artifact before execution.

```text
 OpenAPI JSON/YAML                 HCL modules
        |                              |
        v                              v
 simile-openapi                   simile-hcl parser
  3.0 adapter                      syntax AST
  3.1 adapter                           |
  3.2 adapter                           v
        |                         semantic compiler
        +-------------+----------------+
                      |
                      v
               simile-compiler
        cross-source validation + merge
                      |
                      v
              CompiledService
                (simile-spec)
                      |
                      v
              simile-artifact
        indexed IR + compiled body schemas
         + versioned CBOR envelope
                      |
                      v
                 `.similec`
                      |
                      v
                simile-runtime
       request validation + response synthesis
                      ^
                      |
                simile-server
        HTTP routing + listener lifecycle
```

## Units of work

The crates form a dependency flow, not a collection of interchangeable helpers. Source adapters
translate into `simile-spec`; the compiler coordinates them; artifact and schema crates lower the
model; runtime consumes it; adapters present it.

### Canonical model: `simile-spec`

This crate contains the nouns shared across compilation and execution: an API contract, operations,
behaviors, resources, async operations, and a compiled service. It has no knowledge of YAML, HCL,
CLI arguments, or files. That keeps the future runtime independent of provider syntax.

### OpenAPI adapter: `simile-openapi`

The adapter accepts JSON and YAML, identifies the feature line, and dispatches to a version adapter.
OpenAPI patch versions do not create new adapters because the OpenAPI Initiative defines the
`major.minor` pair as the feature set. Simile therefore accepts every patch in the supported 3.0,
3.1, and 3.2 lines while preserving the exact version for diagnostics and reproducibility.

The normalized operation retains the HTTP method, path, optional `operationId`, request body,
responses, and source schema fragments. An operation does not require an `operationId` for
OpenAPI-derived behavior. It does require one before HCL can target it.

### Behavior frontend: `simile-hcl`

The HCL frontend has two phases:

1. Parsing converts HCL syntax into declarations without consulting OpenAPI.
2. Semantic compilation resolves declarations against the canonical API contract.

The semantic phase rejects duplicate declarations, unknown operation IDs, response statuses absent
from OpenAPI, missing async targets, undeclared resources, and provider-specific resource kinds.
Declarations are collected before references are resolved, so modules do not depend on file order.

### Coordinator: `simile-compiler`

The coordinator owns the build graph, not either language. It parses OpenAPI, parses every HCL
module, invokes semantic validation, and assigns each HTTP operation one behavior origin:

- `open_api`: deterministic behavior derived from the HTTP contract;
- `hcl`: a validated overlay that refines that operation.

This is the only unit allowed to combine source languages. It produces a `CompiledService` for the
runtime and leaves source-specific structures behind.

### Application and presentation

`simile-core` reads sources and presents compilation as an application operation. `simile-cli`
only converts arguments into that request and renders the structured report. Neither crate knows
how either source language works.

The filesystem registry is also a `simile-core` boundary. Registration compiles and validates a
service before installing an immutable revision containing the source HCL, a versioned JSON
manifest, and the `.similec` runtime artifact. The registry atomically advances a small `current`
pointer; it does not maintain a second central index. Starting by name loads and validates the
selected artifact without returning to OpenAPI or HCL. CLI adapters do not construct registry paths
or interpret manifests. Updating is a source operation: core loads either the stored HCL snapshot
or a replacement file or URL, resolves its provenance, compiles a complete artifact, installs a
new revision, and only then advances `current`. The registry never exposes a partially compiled
update to `start`.

### Runtime artifact: `simile-artifact`

The source model is intentionally convenient for parsing and diagnostics; it is not persisted as
JSON. `simile-artifact` lowers `CompiledService` into an indexed representation with a sorted,
deduplicated string table, ordered operation records, numeric references between behaviors,
async operations and resources, compiled request- and response-body plans, and an arena for
retained response definition value trees.

The resulting payload is encoded as deterministic CBOR. A fixed envelope supplies a magic value,
format version, flags, payload length, and SHA-256 payload digest. The reader validates the entire
envelope and every internal index before an artifact can reach the runtime. The binary format is a
compiler output: providers continue to publish and review OpenAPI and HCL as text.

### Schema compiler: `simile-schema`

`simile-schema` owns the executable subset of JSON Schema used for body validation and synthesis.
It compiles each declared request and response media type into an indexed graph, resolves local
component references once per request/response direction, and preserves recursive references
without copying their source trees. Validation operates on a decoded JSON value and returns
bounded, structured issues whose paths are JSON Pointers.

Within that crate, the `soundness` module owns cross-field relationship syntax, compiled relation
types, proof evaluation, and synthesis establishment. The general compiler, validator, and
synthesizer delegate those responsibilities rather than implementing relation-specific branches.

This boundary is intentionally independent of HTTP and OpenAPI parsing. The artifact embeds the
compiled graph; the runtime never interprets source schema fragments. See
[request validation](request-validation.md) for the supported keywords and failure model.
Cross-field relationships use the same graph and validator. Ordering, relative bounds,
implications, equality, equations, aggregates, distinct selectors, scalar set relationships,
templates, and lifecycle snapshots are described in
[payload soundness](payload-soundness.md).

### Generation inference: `simile-heuristics`

`simile-heuristics` owns provider-neutral, best-effort inference from exact field names and
compatible OpenAPI types. The schema compiler stores its typed plans in the artifact; synthesis
applies them transactionally and accepts the result only when the authoritative schema validator
still succeeds. The crate has no role in validating incoming payloads. Explicit
`x-simile-constraints` declarations suppress overlapping hints and remain the only cross-field
proof language. See [generation heuristics](generation-heuristics.md).

### Runtime primitives: `simile-runtime`

`simile-runtime` reads only validated artifact records. Its request-body entry points locate an
operation by stable method/path key, negotiate the declared media type, decode JSON, and apply the
compiled schema. Response synthesis selects an exact, ranged, or default response; honors an HCL
status choice when present; and constructs a valid body from the compiled response schema. When a
validated request is available, compatible values at the same structural path are projected into
the response while generated response-only fields remain intact. Every synthesized value is sent
back through its compiled validator. Missing bodies, undeclared media types, malformed payloads,
schema violations, and exhausted synthesis remain distinct errors so the HTTP adapter can map them
deliberately.

`Runtime::execute` is the protocol-neutral coordination boundary. It validates the incoming
body, supplies the validated value to request-aware response synthesis, and returns both the
immediate response and a plan of the HCL-declared asynchronous operations and abstract resources.
It does not listen on a socket or execute those resources. The HTTP adapter owns route matching,
wire limits, and response writing, then delegates the selected operation to this runtime.
See [operation execution](operation-execution.md).

### HTTP adapter: `simile-server`

`simile-server` owns the transport boundary for a long-lived emulator. It builds a route table
from the validated artifact, prefers literal paths over parameter templates, enforces the wire body
limit, normalizes replay identities, and maps runtime outcomes or errors to HTTP responses. It does
not parse OpenAPI or HCL, synthesize values, or execute abstract resources.

`simile-core` prepares a service by parsing the service configuration, resolving its relative
sources, compiling the contract and behavior, and returning an artifact plus a listener report.
`simile-cli` supplies that artifact to `simile-server`, renders the bound address, and owns the
process lifetime.

## Invariants

- OpenAPI remains the single source of truth for the HTTP wire contract.
- HCL can refine behavior but cannot contradict or replace OpenAPI.
- OpenAPI-only services compile successfully.
- Route names never imply state or asynchronous effects.
- HCL references use published `operationId` values, not method/path guesses.
- Resource declarations describe portable capabilities, not production vendors.
- Distributed definitions contain observable behavior, not sensitive production logic.
- Parsing, semantic validation, orchestration, file I/O, and terminal rendering remain separate
  testable units.
- A registered service starts from its validated artifact and never depends on the original source
  path or availability of a remote specification.
- Compiled artifact bytes are deterministic for the same compiled service.
- Binary compatibility is governed by the artifact envelope version, not the Rust layout of
  source-domain structs.
- Readers reject unknown format versions, unsupported flags, corrupt payloads, dangling indexes,
  truncated files, and trailing bytes.
- Request validation never reparses OpenAPI or follows a source reference at runtime.
- Validation issues use stable JSON Pointer paths and named rules.
- Request synthesis never returns a candidate that fails the compiled request validator.
- Response synthesis works without request context and never returns a body that fails the
  compiled response validator.
- A cross-field relationship is executable only after the compiler resolves its selectors, value
  kinds, constructive witnesses, and relationship-specific graph or expression; unsupported or
  unconstructible relationships fail compilation instead of becoming inert annotations. The same
  rule applies recursively to relationships declared on array item schemas.
- Request-aware response synthesis only projects same-path values accepted by the response schema;
  `writeOnly` fields are absent from the response graph.

## Deliberate next boundaries

The current compiler and server establish source ownership, cross-source validation, a runtime
artifact boundary, compiled JSON request validation, seeded request and request-aware response
synthesis, identity-based in-memory replay, protocol-neutral operation coordination, and local HTTP
serving. Subsequent slices can add form and multipart codecs, parameter validation, durable state
actions, and executable asynchronous instructions without collapsing these boundaries.

Read [Compilation pipeline](/compilation-pipeline) for the same architecture as a data journey,
and [Current boundaries](/current-boundaries) for the fidelity not yet implemented.
