---
title: State, identity, and replay
description: Understand what Simile remembers today and what durable resource state still requires.
---

Generated values should vary across independent calls, but identified calls must be stable. Simile
currently provides process-local replay for idempotent mutations and repeated GET identities. It
does not yet implement a durable resource database or infer CRUD semantics from routes.

## Mutation replay

The HTTP adapter turns `Idempotency-Key` into a replay identity scoped to the operation. The first
completed outcome—including its response and async plan—is cached in the running `Runtime`.

A later call with the same operation and key:

- returns the first completed outcome when the validated request body is the same;
- fails with an idempotency conflict when the body differs; and
- never reruns response generation for that identity.

The runtime stores the normalized validated body rather than raw wire bytes, so replay is tied to
the request value that the operation accepted.

## GET replay

For GET requests, the HTTP adapter uses the full URI as a resource replay identity. Repeating the
same operation and URI returns the first outcome for the lifetime of the process. The runtime
accepts a resource identity only for a `GET`; other methods fail rather than being treated as
implicit reads.

This behavior makes repeated reads stable, but it is not a resource store. A preceding POST does
not automatically populate the GET result because OpenAPI does not specify that state transition.

## Lifetime and persistence

Replay lives in memory inside one `Runtime` instance:

- it survives multiple requests handled by one `simile start` process;
- it does not survive a process restart;
- it does not cross separate `simile run` invocations; and
- it is not stored in the registry's reserved `state/` directory.

The service registry persists immutable compiled revisions, not execution history.

## Why state is not inferred

Routes that look like CRUD are not enough to define state. A service may write several records,
derive one resource from another, defer visibility, or expose a projection rather than stored data.
Inferring those rules would create convincing but incorrect tests.

Durable state will require explicit provider-authored actions and deterministic local resource
implementations. Until then, tests can rely on identity replay and request-aware response values,
but not on cross-operation persistence.

See [Operation execution](/operation-execution) for where replay sits in the runtime and
[Dependencies and effects](/dependencies-and-effects) for planned resource work.
