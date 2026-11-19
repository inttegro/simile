---
title: Quickstart
description: Build a minimal service definition, register it, and send a local HTTP request.
---

This walkthrough builds a small Orders service from an OpenAPI contract, registers the compiled
artifact, starts the HTTP emulator, and sends it a request. It uses OpenAPI only so you can see the
baseline before adding behavior overlays.

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
                product:
                  type: string
                  minLength: 1
                quantity:
                  type: integer
                  minimum: 1
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
                  id:
                    type: string
                  product:
                    type: string
                  quantity:
                    type: integer
```

OpenAPI already gives Simile enough information to validate a request and build a valid response.
There is no HCL file yet because there is no additional public behavior to declare.

## 2. Add the service composition

Create `orders.simile.hcl` next to the OpenAPI file:

```hcl
service "orders" {
  listen {
    host = "127.0.0.1"
    port = 4242
  }

  openapi {
    path = "./orders.yaml"
  }
}
```

The configuration answers operational questions—what the service is called, where it listens,
and where its contract comes from. It does not repeat routes or schemas.

## 3. Register and start it

```bash
simile register orders --config ./orders.simile.hcl
simile start orders
```

Registration compiles the source and atomically installs a validated `.similec` artifact under
`~/.simile`. Starting by name loads that artifact; it does not reopen `orders.yaml`.

Keep the server running and use another terminal for the request:

```bash
curl --request POST http://127.0.0.1:4242/orders \
  --header 'Content-Type: application/json' \
  --data '{"product":"beat_pack","quantity":2}'
```

One possible response is:

```json
{
  "id": "p6r2x9k1",
  "product": "beat_pack",
  "quantity": 2
}
```

The exact generated `id` varies. `product` and `quantity` can be projected from the validated
request because the same paths are compatible in the response schema. The complete response is
validated again before it is returned.

## 4. Run an operation without a server

`simile run` is useful for authoring and CI checks:

```bash
simile run ./orders.yaml --operation-id createOrder
```

Because no request was provided, Simile synthesizes one, validates it, executes the operation, and
prints only the pretty JSON response. Pass `--verbose` to see the normalized request, response
metadata, and outcome provenance.

You can also provide a full request document:

```bash
simile run ./orders.yaml \
  --operation-id createOrder \
  --request '{"headers":{"Content-Type":"application/json"},"body":{"product":"jingle","quantity":1}}'
```

The same `--request` option accepts a file path or `-` for standard input.

## 5. Update the registered service

After editing `orders.yaml`, compile a new immutable revision and move the registration to it:

```bash
simile update orders
```

If compilation fails, the current revision remains active. Restart the service process to load the
new artifact.

Next, read [OpenAPI and HCL](/openapi-and-hcl) to add named outcomes and effects, and
[Service lifecycle](/service-lifecycle) to understand what registration stores.
