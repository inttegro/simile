---
title: State, identity, and replay
description: Understand stable operation identity, durable backend state, and exact reproduction.
---

Independent calls should vary; identified calls should not invent a second result. Simile separates
four concerns that are often collapsed into “state”:

1. operation replay keeps an identified public call stable;
2. Stack state persists declared backend consequences;
3. exchange history preserves sanitized local inspection evidence; and
4. reproduction bundles preserve the evidence required to repeat one execution.

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

The file belongs to the registered service identity. After a compatible contract or behavior
update, Simile restores the existing Stack state, initializes newly declared backends, and records
the new artifact digest automatically. State with a different service identity, incompatible
format or protocol, or invalid encoded data still fails safely. `simile remove NAME --confirm NAME`
deletes the registry entry, this durable state, and exchange history, but refuses while another
process owns the state lock.

## Durable exchange history

A loopback service stores sanitized request and response evidence in
`$SIMILE_HOME/state/services/NAME.exchanges.redb`. The browser and terminal consoles restore this
history after a restart, so repeated testing can continue from the same record of observed calls.

Exchange history has no automatic retention limit. Use **Clear history** in the console, or
`DELETE /.simile/exchanges`, when you intend to remove it. Removing a registered service also
deletes its exchange-history database. Request and response bodies keep the inspector's redaction
and 256 KiB capture limit before they reach durable storage.

## Convention-inferred state

When the public contract proves one database, one entity, and one stable identity, Simile infers
ordinary create, read, list, and delete behavior. `POST`, `PUT`, and `PATCH` can commit the successful
response entity; addressed `GET` and POST `/lookup` can retrieve it; collection `GET` and POST
`/page` can list it; and addressed `DELETE` can remove it. Ambiguous projections, composite or
custom lookup keys, multi-entity results, and provider-specific transitions remain
provider-authored.

## Exact reproductions

`simile run --bundle PATH` captures the compiled artifact, normalized request, seed, controls, and
expected response. `simile replay PATH` verifies the digest and re-executes the same evidence. See
[Reproduction bundles](/reproduction-bundles) for the file contract and
[Conformance suites](/conformance-suites) for batch verification.

Use [State inspection](/state-inspection) to inspect resource identities and causal history without
printing stored values by default. Use the [Console](/simile-web) when the service is running and
you want exchanges, state, and deferred work in one workspace.
