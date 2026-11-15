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
        indexed IR + versioned CBOR envelope
                      |
                      v
                 `.similec`
                      |
                      v
                future runtime
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
async operations and resources, and an arena for request and response value trees.

The resulting payload is encoded as deterministic CBOR. A fixed envelope supplies a magic value,
format version, flags, payload length, and SHA-256 payload digest. The reader validates the entire
envelope and every internal index before an artifact can reach the runtime. The binary format is a
compiler output: providers continue to publish and review OpenAPI and HCL as text.

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

## Deliberate next boundaries

The current compiler establishes source ownership, cross-source validation, and the runtime
artifact boundary. Subsequent slices can add `$ref` resolution, a dedicated schema IR, example
selection, deterministic data generation, state actions, scenario inputs, and executable runtime
instructions without collapsing these boundaries.
