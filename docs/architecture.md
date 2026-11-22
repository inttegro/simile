---
title: Architecture
description: A bird's-eye view of how Simile turns a public service contract into a running emulator.
---

Simile has two halves. At **compile time**, it turns one provider-owned HTTP contract—OpenAPI,
Smithy, or semantic TypeScript—and optional behavior HCL into a portable `.similec` artifact. At
**run time**, it loads that artifact and serves the contract over HTTP. The source languages never
leak into request handling.

```mermaid
flowchart TB
  subgraph Sources["Provider-owned sources"]
    direction LR
    Contract["OpenAPI, Smithy, or semantic TypeScript<br/>wire contract"]
    HCL["Behavior HCL<br/>optional refinements"]
  end

  Compile["<b>Compile time</b><br/>source frontends → canonical model → compiler + schema engine"]
  Artifact[("Portable <code>.similec</code> artifact")]
  Run["<b>Run time</b><br/>application layer → service supervisor"]
  Runtime["Operation runtime<br/>validate, select, synthesize"]
  Stack["Simile Stack<br/>effects, state, causality"]
  State[("redb state")]
  HTTP["HTTP adapter<br/>route, decode, and encode"]
  Client["Application under test"]

  Contract --> Compile
  HCL --> Compile
  Compile --> Artifact --> Run
  Run --> Runtime
  Run --> Stack --> State
  Run --> HTTP
  HTTP <-->|"ordinary HTTP"| Client
```

The `.similec` artifact is the seam between the two halves. Compilation can perform expensive
parsing, reference resolution, cross-source checks, and schema indexing once. Execution then works
only with validated, indexed records and never needs the original contract or HCL.

## The architecture in one request

```mermaid
sequenceDiagram
  participant P as Service provider
  participant C as Simile compiler
  participant A as .similec artifact
  participant H as HTTP adapter
  participant S as Service supervisor
  participant R as Operation runtime
  participant K as Simile Stack
  participant T as Application under test

  P->>C: HTTP contract + optional HCL
  C->>C: Normalize, prove, and index
  C->>A: Write deterministic artifact
  S->>A: Load and validate once
  T->>H: HTTP request
  H->>H: Match route and decode transport
  H->>S: normalized operation call
  S->>R: validate, select, synthesize
  R-->>S: committed result + effect plan
  S->>K: transactional synchronous actions
  K-->>S: durable checkpoint
  S-->>H: response + delivery plan
  H-->>T: contract-valid HTTP response
  H-->>S: dispatch named async work
```

This separation lets the same compiled service power a one-shot CLI operation, a local HTTP server,
or a future host without making schema logic depend on any transport.

## Explore the components

| Area | Owns | Detailed page |
| --- | --- | --- |
| Sources and model | Contract frontends, HCL semantics, and the version-neutral service model | [Source model](/source-model) |
| Compilation | Cross-source coordination and lowering into executable records | [Compilation pipeline](/compilation-pipeline) |
| Schema engine | Validation, synthesis, explicit soundness constraints, and cautious inference | [Schema engine](/schema-engine) |
| Artifact | Deterministic CBOR encoding and the compile-time/run-time compatibility boundary | [Compiled artifact](/compiled-artifact) |
| Runtime | Protocol-neutral operation execution | [Operation execution](/operation-execution) |
| Supervisor | Atomic consequence execution and durable service ownership | [Dependencies and effects](/dependencies-and-effects) |
| Stack | Deterministic backends, persistence, and causal journal | [Simile Stack](/simile-stack) |
| HTTP | Routing, wire limits, content negotiation, and error adaptation | [HTTP server](/http-server) |
| Application layer | Source loading, service registration, revisions, and process orchestration | [Application and registry](/application-registry) |

## Rules that preserve the boundaries

- Exactly one OpenAPI, Smithy, or semantic TypeScript source is the source of truth for the public
  HTTP contract. HCL may refine observable behavior, but it cannot replace or contradict it.
- `simile-compiler` is the only layer allowed to combine source languages.
- `.similec` is the only contract between compilation and execution. A registered service starts
  from its artifact rather than refetching its sources.
- `simile-runtime` is protocol-neutral. HTTP routing and response writing belong to
  `simile-server`; backend consequences belong to `simile-service`; files, URLs, registry paths,
  and terminal output belong above all three.
- Explicit `x-simile-constraints` participate in proof and construction. Heuristics may improve
  generated values, but they never decide whether an incoming payload is valid.
- Published behavior describes what an integrator can observe, not private production decision
  trees or provider-specific infrastructure.

See [Capabilities and limits](/current-boundaries) for the current fidelity boundary.
