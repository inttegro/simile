---
title: Quickstart
description: Define, verify, and run a local Orders service in a few minutes.
---

This guide builds one complete service with OpenAPI. You will verify an operation, register the
service, start its HTTP endpoint, and run an application test against it.

Install Simile first, then confirm the binary is ready:

```bash
simile version
simile doctor
```

See [Downloads](/downloads) when Simile is not installed yet.

## 1. Write the public contract

Create `orders.yaml`:

```yaml
openapi: 3.1.0
info:
  title: Orders
  version: 1.0.0
paths:
  /orders:
    post:
      operationId: createOrder
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              additionalProperties: false
              required: [product, quantity]
              properties:
                product: { type: string, minLength: 1 }
                quantity: { type: integer, minimum: 1 }
      responses:
        "201":
          description: Order created
          content:
            application/json:
              schema:
                type: object
                additionalProperties: false
                required: [id, product, quantity]
                properties:
                  id: { type: string, pattern: "^or_[a-z0-9]{12}$" }
                  product: { type: string }
                  quantity: { type: integer }
              x-simile-from-request:
                bindings:
                  - { from: /product, to: /product }
                  - { from: /quantity, to: /quantity }
```

The extension makes `product` and `quantity` hard request-to-response postconditions. Simile may
generate the order ID, while the response must preserve the values supplied by the caller.

## 2. Compose the service

Create `Simfile` beside the contract:

```hcl
service "orders" {
  description = "A local Orders service for integration tests."

  listen {
    host = "127.0.0.1"
    port = 4242
  }

  openapi {
    path = "./orders.yaml"
  }
}
```

The contract owns the HTTP surface. The `Simfile` names the runnable service, selects its contract,
and sets its local listener.

## 3. Verify one operation

Describe the assembled service, then execute `createOrder` directly:

```bash
simile describe
simile run ./orders.yaml \
  --operation-id createOrder \
  --request '{"body":{"product":"beat_pack","quantity":2}}'
```

`run` validates the request, constructs a reachable response, proves the declared relation, and
prints the response payload. Add `--verbose` to see the normalized request, selected outcome, and
response metadata.

## 4. Register and start

Registration compiles the service into an immutable local revision:

```bash
simile register
simile start orders
```

Keep the server running. In another terminal, call it as an ordinary HTTP service:

```bash
curl --request POST http://127.0.0.1:4242/orders \
  --header 'Content-Type: application/json' \
  --data '{"product":"beat_pack","quantity":2}'
```

One response may look like:

```json
{
  "id": "or_k7m2p9x4q1vz",
  "product": "beat_pack",
  "quantity": 2
}
```

Generated values vary between new requests. Idempotency and resource identity can make repeated
calls replay the first committed result when the contract supports those identities.

## 5. Run an application test

Let Simile own the emulator's lifetime during a test:

```bash
simile test orders -- npm test
```

Simile starts the service on an available port, exports that address as `SIMILE_BASE_URL`, runs the
test command, returns its exit status, and shuts the service down.

## Continue

- [Use a service](/service-lifecycle) explains registration, setup, profiles, updates, and removal.
- [Define a service](/define-a-service) is the complete provider authoring workflow.
- [Contract sources and HCL](/contract-sources-and-hcl) covers OpenAPI, Smithy, semantic TypeScript,
  and optional public behavior.
- [Local testing](/local-testing) and [CI testing](/ci-testing) cover application test environments.
