---
title: Architecture
description: A bird's-eye view of how Simile turns a public service contract into a running emulator.
---

Simile has two halves. At **compile time**, it turns provider-owned OpenAPI and optional behavior
HCL into a portable `.similec` artifact. At **run time**, it loads that artifact and serves the
contract over HTTP. The source languages never leak into request handling.

```mermaid
flowchart TB
  subgraph Sources["Provider-owned sources"]
    direction LR
    OA["OpenAPI<br/>wire contract"]
    HCL["Behavior HCL<br/>optional refinements"]
  end

  Compile["<b>Compile time</b><br/>source frontends → canonical model → compiler + schema engine"]
  Artifact[("Portable <code>.similec</code> artifact")]
  Run["<b>Run time</b><br/>application layer → operation runtime"]
  HTTP["HTTP adapter<br/>route, decode, and encode"]
  Client["Application under test"]

  OA --> Compile
  HCL --> Compile
  Compile --> Artifact --> Run --> HTTP
  HTTP <-->|"ordinary HTTP"| Client
```

The `.similec` artifact is the seam between the two halves. Compilation can perform expensive
parsing, reference resolution, cross-source checks, and schema indexing once. Execution then works
only with validated, indexed records and never needs the original OpenAPI or HCL.

## The architecture in one request

```mermaid
sequenceDiagram
  participant P as Service provider
  participant C as Simile compiler
  participant A as .similec artifact
  participant S as Simile server
  participant T as Application under test

  P->>C: OpenAPI + optional HCL
  C->>C: Normalize, prove, and index
  C->>A: Write deterministic artifact
  S->>A: Load and validate once
  T->>S: HTTP request
  S->>S: Match route and decode transport
  S->>S: Validate request and select outcome
  S-->>T: Contract-valid HTTP response
```

This separation lets the same compiled service power a one-shot CLI operation, a local HTTP server,
or a future host without making schema logic depend on any transport.

## Explore the components

| Area | Owns | Detailed page |
| --- | --- | --- |
| Sources and model | OpenAPI adaptation, HCL semantics, and the version-neutral service model | [Source model](/source-model) |
| Compilation | Cross-source coordination and lowering into executable records | [Compilation pipeline](/compilation-pipeline) |
| Schema engine | Validation, synthesis, explicit soundness constraints, and cautious inference | [Schema engine](/schema-engine) |
| Artifact | Deterministic CBOR encoding and the compile-time/run-time compatibility boundary | [Compiled artifact](/compiled-artifact) |
| Runtime | Protocol-neutral operation execution | [Operation execution](/operation-execution) |
| HTTP | Routing, wire limits, content negotiation, and error adaptation | [HTTP server](/http-server) |
| Application layer | Source loading, service registration, revisions, and process orchestration | [Application and registry](/application-registry) |

## Rules that preserve the boundaries

- OpenAPI is the source of truth for the public HTTP contract. HCL may refine observable behavior,
  but it cannot replace or contradict that contract.
- `simile-compiler` is the only layer allowed to combine source languages.
- `.similec` is the only contract between compilation and execution. A registered service starts
  from its artifact rather than refetching its sources.
- `simile-runtime` is protocol-neutral. HTTP routing and response writing belong to
  `simile-server`; files, URLs, registry paths, and terminal output belong above both.
- Explicit `x-simile-constraints` participate in proof and construction. Heuristics may improve
  generated values, but they never decide whether an incoming payload is valid.
- Published behavior describes what an integrator can observe, not private production decision
  trees or provider-specific infrastructure.

See [Current boundaries](/current-boundaries) for the fidelity Simile deliberately does not claim
yet.
