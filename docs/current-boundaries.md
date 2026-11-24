---
title: Capabilities and limits
sidebarTitle: Capabilities
description: See what Simile v0.5.0 can emulate, where provider authorship is required, and which boundaries remain explicit.
---

Simile v0.5.0 runs stateful HTTP service emulators from public contracts. It compiles the service
before startup, validates each request, constructs a reachable result, commits declared
consequences, and exposes the service through an ordinary local HTTP endpoint.

This page describes the current product boundary. The [CLI reference](/cli-reference) covers
commands, while [Architecture](/architecture) explains how the components enforce these promises.

## Capability map

| Area | Current support |
| --- | --- |
| Contract sources | OpenAPI 3.0, 3.1, and 3.2; Smithy 2.0 HTTP services; declarations-only semantic TypeScript. |
| Requests | Path, query, header, cookie, JSON, form URL-encoded, multipart, and text decoding with compiled validation. Simile can synthesize a valid request when one is omitted. |
| Responses | Schema-valid and request-aware JSON, text, form, multipart, binary, bodyless, and redirect outcomes with final proof before delivery. |
| Public behavior | Named outcomes, reusable error sets, request predicates, opaque scenarios, protocol errors, request-to-response relations, and payload soundness constraints. |
| State | Durable database, cache, queue, mail, object store, event bus, scheduler, outbound HTTP, logical time, and causal journal capabilities through Simile Stack. |
| Consequences | Transactional synchronous effects, durable asynchronous work, retries, crash recovery, and dead-letter storage. |
| Authentication | API key, Basic, Bearer, OAuth 2.0, and OpenID Connect test principals, setup workflows, grants, profiles, and credential lifecycle states. |
| Webhooks | OpenAPI callbacks and webhooks, HCL contracts, committed emissions, causal dependencies, delays, ordering policy, HMAC-SHA256 signing, retries, and durable delivery. |
| Failure testing | Deterministic scenarios and bounded stochastic transport-fault profiles. |
| Inspection | Terminal and browser consoles over registered artifacts, sanitized durable exchange history, live Stack state, deferred work, and fidelity evidence. |
| Reproduction | Seeded runs, self-contained bundles, replay, conformance suites, causal inspection, and verified issue reports. |
| Learning | Buffered local HTTP proxying with sanitized evidence and conservative draft contract inference. |

## Sources of authority

Every behavior has a visible source:

1. **Contract-derived** behavior comes from OpenAPI, Smithy, or semantic TypeScript.
2. **Provider-authored** behavior comes from public HCL outcomes, effects, scenarios, faults, and
   setup declarations.
3. **Convention-inferred** behavior covers conservative cases such as one unambiguous database
   entity and ordinary create, read, list, or delete operations.
4. **Observed** behavior comes from sanitized proxy evidence and remains a draft until a provider
   reviews it.

`simile describe service --verbose` shows the assembled model. `simile audit` identifies gaps,
unconstrained synthesis, and inferred behavior that still needs review.

## Where provider authorship is required

Simile deliberately asks the provider to describe public behavior when the contract cannot prove
it. Common examples include:

- lifecycle transitions controlled by public request fields;
- operation-specific errors and their public triggers;
- multi-entity mutations or custom lookup keys;
- totals, currencies, eligibility, or optional data that must reconcile across a payload;
- background work, outbound calls, and webhook emissions; and
- authentication setup that issues local credentials or collects caller configuration.

Use [`x-simile-from-request`](/request-response-relations) and
[`x-simile-constraints`](/x-simile-constraints) for payload relationships. Use HCL for public
outcomes and consequences. Private risk models and internal implementation decision trees do not
belong in either source.

## Explicit limits

### Transport

The built-in server handles buffered HTTP requests. TLS termination, mutual TLS identity,
WebSockets, server-sent events, `CONNECT` tunnels, and streaming bodies require another adapter or
remain future work. Unsupported media types and parameter styles fail clearly during compilation or
request handling.

### State and replay

Simile persists Stack state, deferred work, profiles, and reports under `SIMILE_HOME`. Automatic
CRUD inference applies only when the entity, database, and identity are unambiguous. More complex
state transitions need authored effects or state projections.

Operation response replay for idempotency keys and GET resource identities currently belongs to
one running service process. Reproduction bundles remain portable across processes because they
carry the compiled artifact, request, and seed.

### Proxy learning

`simile proxy --learn` is designed for local development. It buffers request and response bodies,
limits each body to 16 MiB, redacts recognized credentials, and may retain business values needed
for inference. Review its evidence directory before sharing it. Production capture and transparent
streaming proxy support are outside the current workflow.

### Production fidelity

Simile can reproduce published behavior and provider-reviewed examples. It cannot discover private
fraud decisions, undocumented capacity limits, production topology, or measured provider latency
from a schema. Model only the behavior that an integration is entitled to rely on.

## A practical testing boundary

Use Simile to test contract validation, reachable success and error outcomes, authentication,
idempotency, stateful resource flows, asynchronous consequences, webhooks, transport faults, and
recovery after restart. Keep a smaller set of provider-hosted tests for production credentials,
network policy, account configuration, and behavior the public definition does not cover.
