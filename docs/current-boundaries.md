---
title: Current boundaries
description: Know which parts of service emulation are implemented and which remain explicit future layers.
---

Simile is useful only when its fidelity is stated honestly. The current implementation can compile
and run a body-oriented HTTP contract, but it is not yet the complete stateful and asynchronous
service described by the long-term charter.

## Implemented now

### Source and compilation

- OpenAPI 3.0, 3.1, and 3.2 ingestion from JSON or YAML
- Smithy 2.0 IDL ingestion for HTTP services
- declarations-only semantic TypeScript ingestion, including split packages and relative imports
- optional HCL behavior modules and service configuration
- named outcomes, reusable error sets, public request predicates, and opaque scenarios
- backend and async-operation declarations
- supported JSON Schema compilation, explicit soundness constraints, and generation heuristics
- deterministic, versioned `.similec` artifacts with integrity validation
- compiled path, query, header, and cookie parameter contracts
- typed synchronous effects, named asynchronous units, and conservative database-write inference
- OpenAPI callback and OpenAPI 3.1/3.2 top-level webhook ingestion
- provider-authored webhook contracts and compact multi-webhook effect emissions

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
- compiled best-effort or strict webhook ordering policy with bounded, reproducible pair decisions
- schema-checked webhook occurrence planning with body-derived callback targets, hard dependencies,
  explicit delays, and destination overrides
- response delay before delivery and disconnect after commit
- resolved planning of HCL-declared async work
- transactional synchronous effect execution with durable Stack checkpoints
- background dispatch of named asynchronous units without response-path waiting
- deterministic reproduction bundles and batch conformance replay

### Stack backend layer

- typed deterministic engines for database, cache, queue, mail, object store, event bus,
  scheduler, and outbound HTTP capabilities
- one versioned command/result protocol with backend-kind validation and idempotent replay
- one logical-time model for cache TTLs, queue visibility, scheduled work, and timestamps
- serializable snapshots, reset, stable backend errors, and a payload-free causal journal
- separately runnable `simile-*` component processes with uniform lifecycle endpoints
- automatic redb checkpoints under `SIMILE_HOME` that restore complete component state, time,
  journal, counters, and replay records after restart
- read-only state and causal-journal inspection with explicit payload opt-in

### HTTP server

- literal and `{parameter}` path matching, preferring literal routes
- `404` and `405` transport responses
- an 8 MiB body limit
- request body content-type handling
- compiled-schema validation for path, query, header, and cookie parameters
- one process-wide opaque scenario selection
- one optional process-wide stochastic fault profile
- one-line safe access logs and graceful Ctrl-C shutdown

## Not implemented yet

### Request surface

Authentication behavior is not emulated. Form URL-encoded, multipart, and other non-JSON body
codecs are not decoded. Less common OpenAPI serialization styles that the adapter cannot execute
are reported by `simile audit` rather than silently approximated.

### Stateful behavior

The service supervisor persists provider-authored effects and conservatively inferred ordinary
database writes. It does not yet infer a full CRUD algebra from route names: a later GET is not
automatically served from the record written by a POST, and ambiguous DELETE or projection
semantics still require provider authorship. Operation-response replay remains process-local.

### Effect execution

Synchronous effects are transactional and named asynchronous units dispatch outside the response
critical path. The current background dispatcher is process-owned: it does not yet provide a durable
outbox, lease, retry policy, or crash recovery for work that was committed immediately before the
process stopped. Providers should not use completion of async actions as evidence of a production
delivery guarantee.

### Webhook delivery

Simile extracts OpenAPI callbacks and OpenAPI 3.1/3.2 top-level webhooks, compiles provider-authored
fallback contracts, and plans schema-checked occurrences from committed effects. It resolves body
pointer callback targets, explicit destination overrides, delays, and hard causal dependencies.
The service-wide policy can then make bounded, reproducible ordering decisions for independent
occurrences.

The current server does not yet place those occurrences in a durable outbox, deliver signed HTTP
requests, persist retry attempts, or expose redelivery commands. A planned occurrence is therefore
proof of what the operation caused, not evidence that a receiver accepted it. The `webhooks` block
configures delivery semantics but does not by itself send a request.

### Production equivalence

Simile does not infer provider latency, capacity, private risk decisions, undocumented behavior,
or infrastructure. It can apply explicitly authored delays and disconnect-after-commit faults, but
those controls are test scenarios—not measurements of production behavior. Schema-synthesized
outcomes are contract-valid approximations, not evidence of exact production bodies.

## How to write tests today

You can confidently test:

- whether your JSON request body satisfies the published supported schema;
- whether supported path, query, header, and cookie values satisfy their schemas;
- how your integration handles declared success and error bodies;
- public scenarios published by the provider;
- same-path values returned from a request-aware response;
- request-constrained results when the provider publishes `x-simile-from-request`;
- idempotency conflict handling and stable process-local replay;
- how an integration handles declared latency or a lost response after commit; and
- whether synchronous backend consequences commit atomically; and
- which named post-response actions a definition dispatches; and
- which webhook occurrences a committed effect plans, with schema-valid payloads and causal edges.

Do not yet assert that ordinary GET/DELETE semantics were inferred from route shape, post-response
work survived a process crash, or a planned webhook was accepted by a network receiver. Those are
still explicit boundaries rather than plausible claims.

Read [Outcome fidelity](/outcome-fidelity) to distinguish exact evidence from synthesis and
[Fault injection](/fault-injection) for the environmental failure model, or
[State, identity, and replay](/state-and-replay) for the current persistence model.
