---
title: Current boundaries
description: Know which parts of service emulation are implemented and which remain explicit future layers.
---

Simile is useful only when its fidelity is stated honestly. The current implementation can compile
and run a body-oriented HTTP contract, but it is not yet the complete stateful and asynchronous
service described by the long-term charter.

## Implemented now

### Source and compilation

- OpenAPI 3.0, 3.1, and 3.2 feature-line ingestion from JSON or YAML
- optional HCL behavior modules and service configuration
- named outcomes, reusable error sets, public request predicates, and opaque scenarios
- abstract resource and async-operation declarations
- supported JSON Schema compilation, explicit soundness constraints, and generation heuristics
- deterministic, versioned `.similec` artifacts with integrity validation

### Operation runtime

- operation lookup by stable key or `operationId`
- JSON request body decoding and compiled-schema validation
- request synthesis when `simile run` receives no request
- response selection and schema-valid synthesis
- structural request-to-response value projection
- provider-declared request-to-response bindings, derived totals, reachable lifecycle rules, and
  optional-data eligibility proofs
- provider-declared protocol-error rendering, with Simile diagnostics as a fallback
- process-local replay by idempotency key and GET URI
- deterministic scenario faults and bounded, seeded stochastic fault profiles
- response delay before delivery and disconnect after commit
- resolved planning of HCL-declared async work

### Stack resource layer

- typed deterministic engines for database, cache, queue, mail, object store, event bus,
  scheduler, and outbound HTTP capabilities
- one versioned command/result protocol with resource-kind validation and idempotent replay
- one logical-time model for cache TTLs, queue visibility, scheduled work, and timestamps
- serializable snapshots, reset, stable resource errors, and a payload-free causal journal
- separately runnable `simile-*` component processes with uniform lifecycle endpoints

### HTTP server

- literal and `{parameter}` path matching, preferring literal routes
- `404` and `405` transport responses
- an 8 MiB body limit
- request body content-type handling
- one process-wide opaque scenario selection
- one optional process-wide stochastic fault profile
- one-line safe access logs and graceful Ctrl-C shutdown

## Not implemented yet

### Request surface

Path, query, and header parameter schemas are not compiled or validated. Authentication behavior
is not emulated. Form URL-encoded, multipart, and other non-JSON body codecs are not decoded.

### Stateful behavior

The Stack engines can retain and snapshot resource state, but the operation runtime does not yet
compile or dispatch provider-authored state actions. A POST served by `simile start` therefore does
not implicitly create a resource for a later GET. Operation replay remains process-local.

### Effect execution

HCL async operations and their abstract dependencies are retained for discovery, but are not
attached to operation results. The HTTP server does not launch or dispatch Stack components. The
components already provide local state, a causal journal, logical time, and manual advancement; the
missing layer is the provider-authored effect and action IR plus the adapter that sends commands
after operation commit.

### Production equivalence

Simile does not infer provider latency, capacity, private risk decisions, undocumented behavior,
or infrastructure. It can apply explicitly authored delays and disconnect-after-commit faults, but
those controls are test scenarios—not measurements of production behavior. Schema-synthesized
outcomes are contract-valid approximations, not evidence of exact production bodies.

## How to write tests today

You can confidently test:

- whether your JSON request body satisfies the published supported schema;
- how your integration handles declared success and error bodies;
- public scenarios published by the provider;
- same-path values returned from a request-aware response;
- request-constrained results when the provider publishes `x-simile-from-request`;
- idempotency conflict handling and stable process-local replay;
- how an integration handles declared latency or a lost response after commit; and
- which follow-up operations the definition declares as planned.

Do not yet assert through `simile start` that cross-operation resource state changed or that a
declared async effect was delivered. Stack can model both in isolation, but automatic operation
integration is still a documented boundary.

Read [Outcome fidelity](/outcome-fidelity) to distinguish exact evidence from synthesis and
[Fault injection](/fault-injection) for the environmental failure model, or
[State, identity, and replay](/state-and-replay) for the current persistence model.
