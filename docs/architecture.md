# Compiler architecture

Simile compiles two provider-authored sources into one runtime contract. OpenAPI is mandatory and
complete for HTTP. HCL is optional and additive for behavior.

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
```

## Units of work

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

This boundary is intentionally independent of HTTP and OpenAPI parsing. The artifact embeds the
compiled graph; the runtime never interprets source schema fragments. See
[request validation](request-validation.md) for the supported keywords and failure model.

### Runtime primitives: `simile-runtime`

`simile-runtime` reads only validated artifact records. Its request-body entry points locate an
operation by stable method/path key, negotiate the declared media type, decode JSON, and apply the
compiled schema. Response synthesis selects an exact, ranged, or default response; honors an HCL
status choice when present; and constructs a valid body from the compiled response schema. When a
validated request is available, compatible values at the same structural path are projected into
the response while generated response-only fields remain intact. Every synthesized value is sent
back through its compiled validator. Missing bodies, undeclared media types, malformed payloads,
schema violations, and exhausted synthesis remain distinct errors so a future HTTP adapter can map
them deliberately.

`Runtime::execute` is the protocol-neutral coordination boundary. It validates the incoming
body, supplies the validated value to request-aware response synthesis, and returns both the
immediate response and a plan of the HCL-declared asynchronous operations and abstract resources.
It does not listen on a socket or execute those resources. A future HTTP adapter will own route
matching, wire limits, and response writing, then delegate the selected operation to this runtime.
See [operation execution](operation-execution.md).

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
- Request-aware response synthesis only projects same-path values accepted by the response schema;
  `writeOnly` fields are absent from the response graph.

## Deliberate next boundaries

The current compiler establishes source ownership, cross-source validation, a runtime artifact
boundary, compiled JSON request validation, deterministic request synthesis, deterministic
request-aware response synthesis, and protocol-neutral operation coordination. Subsequent slices
can add form and multipart codecs, parameter validation, state actions, scenario inputs,
executable asynchronous instructions, and HTTP serving without collapsing these boundaries.
