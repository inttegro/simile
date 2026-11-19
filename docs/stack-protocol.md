---
title: Protocol
description: The common command, result, causality, and lifecycle contract for Stack components.
---

Every Stack component speaks one exhaustive, versioned internal protocol. The protocol is shared
because a service operation should not need a different lifecycle model for a queue than for a
database or scheduler.

## Resource addresses

A resource address combines a capability kind and a provider-chosen namespace:

```text
db.orders
queue.fulfillment
mail.receipts
```

Only declared addresses may receive commands. A database command sent to `queue.fulfillment` is a
kind mismatch rather than an implicit conversion.

## Command envelope

The supervisor assigns the command identity and logical time before dispatch:

```json
{
  "protocolVersion": 1,
  "resource": { "kind": "queue", "name": "fulfillment" },
  "context": {
    "commandId": 42,
    "at": 1732147200000,
    "metadata": {
      "operationId": "createOrder",
      "causationId": "request_8f2a",
      "correlationId": "order_or_123",
      "idempotencyKey": "checkout_4c9b"
    }
  },
  "command": {
    "component": "queue",
    "command": {
      "action": "publish",
      "body": { "orderId": "or_123" },
      "attributes": {},
      "delayMs": 0
    }
  }
}
```

The result repeats the assigned identity and time. Component result variants are exhaustive, so a
protocol-version change is required before a new command family can silently appear.

## Causality and idempotency

The metadata fields answer different questions:

| Field | Question |
| --- | --- |
| `operationId` | Which API operation planned this command? |
| `causationId` | Which request or prior event directly caused it? |
| `correlationId` | Which wider business flow does it belong to? |
| `idempotencyKey` | Has this exact resource command already completed? |

Reusing an idempotency key for the same resource and command returns the original result with
`replayed: true`. Reusing it for different input fails with `conflict`; Stack never guesses which
command the caller intended.

## Stable errors

Components return a small shared error vocabulary: `conflict`, `invalid_command`, `invalid_value`,
`kind_mismatch`, `not_found`, `undeclared_resource`, and `unavailable`. Human-readable messages add
context, but callers can branch on the stable code.

## Component process surface

Each `simile-*` component process exposes the same lifecycle routes:

| Route | Purpose |
| --- | --- |
| `GET /v1/manifest` | Protocol version, component kind, and capabilities |
| `GET /v1/health` | Readiness |
| `POST /v1/commands` | Execute one command envelope |
| `POST /v1/events` | Query payload-free journal entries |
| `POST /v1/time/advance` | Advance a manual clock and release due work |
| `POST /v1/reset` | Return to initial empty state |
| `GET /v1/snapshot` | Serialize deterministic state |
| `POST /v1/restore` | Restore a compatible snapshot |

The process writes one JSON readiness record to stdout. Diagnostics go to stderr, which makes the
process safe to supervise and compose without parsing human log text.
