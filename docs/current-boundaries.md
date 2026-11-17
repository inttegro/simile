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
- provider-declared protocol-error rendering, with Simile diagnostics as a fallback
- process-local replay by idempotency key and GET URI
- deterministic scenario faults and bounded, seeded stochastic fault profiles
- response delay before delivery and disconnect after commit
- resolved planning of HCL-declared async work

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

There is no durable resource store or provider-authored state-action language yet. A POST does not
implicitly create a resource for a later GET. Replay exists only in memory for one runtime process.

### Effect execution

HCL async operations are returned as resolved plans, but the HTTP server does not dispatch queues,
mail, webhooks, schedules, databases, or event buses. There is no effect history, retry clock, or
manual async advancement interface yet.

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
- idempotency conflict handling and stable process-local replay;
- how an integration handles declared latency or a lost response after commit; and
- which follow-up operations the definition declares as planned.

Do not yet assert that cross-operation resource state changed or that a declared async effect was
delivered. Those would convert a documented future boundary into a false test guarantee.

Read [Outcome fidelity](/outcome-fidelity) to distinguish exact evidence from synthesis and
[Fault injection](/fault-injection) for the environmental failure model, or
[State, identity, and replay](/state-and-replay) for the current persistence model.
