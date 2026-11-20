---
title: State, identity, and replay
description: Understand stable operation identity, durable backend state, and exact reproduction.
---

Independent calls should vary; identified calls should not invent a second result. Simile separates
three concerns that are often collapsed into “state”:

1. operation replay keeps an identified public call stable;
2. Stack state persists declared backend consequences; and
3. reproduction bundles preserve the evidence required to repeat one execution.

## Operation replay

The HTTP adapter turns `Idempotency-Key` into a replay identity scoped to the operation. A later
call with the same operation and key returns the committed outcome when the validated request is
the same, and fails with an idempotency conflict when the request differs. Effects are not repeated
for a replayed operation.

For GET requests, the adapter uses the full URI as a resource identity. Repeating the same operation
and URI returns the first outcome for the lifetime of that operation runtime.

Operation response replay remains process-local. It protects retries during one running service;
durable backend state and cross-process response caching have separate lifecycles.

## Durable backend state

Effects send typed commands to Simile Stack. The service supervisor applies each synchronous unit
transactionally, saves the complete Stack snapshot to embedded redb, and rolls back in-memory state
if the command or checkpoint fails. A restarted service restores backend state, logical time,
causal journal, counters, and Stack command replays from `$SIMILE_HOME/state/services/NAME.redb`.

The file is bound to the registered service identity and artifact digest. Simile refuses to load it
for a different or incompatible compiled service. `simile remove NAME --confirm NAME` deletes both
the registry entry and this durable state, but refuses while another process owns the state lock.

## Convention-inferred writes

When the public contract proves one ordinary write—one database, stable operation ID, one
identifiable successful entity—Simile infers a `db.put` effect for `POST`, `PUT`, and `PATCH`.
Ambiguous transitions remain provider-authored. Simile does not infer that every route-shaped GET
is a database read or that every DELETE removes one collection record.

## Exact reproductions

`simile run --bundle PATH` captures the compiled artifact, normalized request, seed, controls, and
expected response. `simile replay PATH` verifies the digest and re-executes the same evidence. See
[Reproduction bundles](/reproduction-bundles) for the file contract and
[Conformance suites](/conformance-suites) for batch verification.

Use [State inspection](/state-inspection) to inspect resource identities and causal history without
printing stored values by default.
