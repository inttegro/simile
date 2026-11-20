---
title: Current boundaries
description: Know which parts of service emulation are implemented and which remain explicit future layers.
---

Simile earns trust by stating the authority behind each result. The current implementation runs
stateful HTTP services with durable consequences, contract authentication, and outbound webhooks.
Several production behaviors still require explicit provider authorship or remain outside the
emulator.

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
- JSON, form URL-encoded, multipart form-data, and text request decoding with compiled-schema validation
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
- a durable work outbox committed atomically with operation state
- leased background dispatch with bounded retries, crash recovery, and dead-letter storage
- signed outbound webhook delivery over HTTP with retry and redelivery primitives
- API key, Basic, Bearer, OAuth 2.0, and OpenID Connect test-principal authentication
- conservative database-backed create, read, list, and delete behavior when one entity is unambiguous
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
- OpenAPI serialization for simple, label, matrix, form, space-delimited, pipe-delimited, and
  deep-object parameters where their locations permit them
- one process-wide opaque scenario selection
- one optional process-wide stochastic fault profile
- one-line safe access logs and graceful Ctrl-C shutdown

## Deliberate boundaries

### Request surface

OpenAPI security requirements are enforced with local HCL principal contracts and profiles. Mutual TLS still needs
a TLS-aware listener capable of exposing verified client identity to the runtime. Unsupported
parameter styles remain audit errors, and media types without a codec fail explicitly.

### Stateful behavior

The service supervisor infers ordinary create, read, list, and delete behavior only when the
contract identifies one database, one entity, and one stable identity. Ambiguous projections,
multi-entity responses, custom lookup keys, and provider-specific transitions still require an
authored effect. Operation-response replay remains process-local, while Stack state and deferred
work survive restarts.

### Effect execution

Synchronous effects, the outbox publication, and the Stack checkpoint share one commit boundary.
Workers lease deferred work, retry failures, and move exhausted items to durable dead-letter
storage. The service library exposes inspection and redelivery primitives. Dead-letter operations
currently use that library API; a dedicated operator CLI is a future interface.

### Webhook delivery

Simile extracts OpenAPI callbacks and OpenAPI 3.1/3.2 top-level webhooks, compiles provider-authored
fallback contracts, and plans schema-checked occurrences from committed effects. It resolves body
pointer callback targets, explicit destination overrides, delays, and hard causal dependencies.
The service-wide policy can then make bounded, reproducible ordering decisions for independent
occurrences.

Committed occurrences enter the durable outbox. The worker applies hard dependencies and the
service ordering policy, sends HTTP requests, attaches configured HMAC-SHA256 signatures, and
acknowledges only successful responses. Retry exhaustion moves the occurrence to dead-letter
storage. Delivery logs and durable state show emulator activity; they do not establish behavior of
the provider's production webhook infrastructure.

### Production equivalence

Provider latency, capacity, private risk decisions, undocumented behavior, and infrastructure need
explicit public evidence before Simile can reproduce them. Explicit delays and
disconnect-after-commit faults are testing controls rather than measurements. Schema-synthesized
outcomes carry contract-derived provenance. Authored outcomes and effects carry provider-authored
provenance. Convention-derived state behavior remains visibly inferred in `simile audit` and
`simile describe service --verbose`.

## How to write tests today

You can confidently test:

- whether a supported JSON, form, multipart, or text request satisfies the published schema;
- whether supported path, query, header, and cookie values satisfy their schemas;
- how your integration handles declared success and error bodies;
- public scenarios published by the provider;
- same-path values returned from a request-aware response;
- request-constrained results when the provider publishes `x-simile-from-request`;
- idempotency conflict handling and stable process-local replay;
- how an integration handles declared latency or a lost response after commit; and
- whether declared credentials and grants produce `401`, `403`, or operation execution;
- whether inferred and authored database consequences survive restart;
- whether synchronous backend consequences and deferred-work publication commit atomically;
- whether named post-response actions retry and recover after restart; and
- whether webhook occurrences deliver schema-valid, optionally signed payloads with causal edges.

Do not treat convention inference as provider authorship, a successful local webhook as production
acceptance, or a synthesized payload as evidence of an undocumented provider decision.

Read [Outcome fidelity](/outcome-fidelity) to distinguish exact evidence from synthesis and
[Fault injection](/fault-injection) for the environmental failure model, or
[State, identity, and replay](/state-and-replay) for the current persistence model.
