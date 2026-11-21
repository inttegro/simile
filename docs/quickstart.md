---
title: Quickstart
description: Build a minimal service definition, register it, and send a local HTTP request.
---

This walkthrough builds a small Orders service, registers the compiled artifact, starts the HTTP
emulator, and sends it a request. Choose OpenAPI, Smithy, or semantic TypeScript; each source below
defines the same public operation and compiles into the same runtime model.

## 1. Choose the public contract

Use one of the following contract files. You do not need to maintain all three.

### OpenAPI

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

### Smithy

Create `orders.smithy`:

```smithy
$version: "2.0"
namespace example.orders

@trait(selector: "member")
string contentType

@trait(selector: "operation")
document responseRelations

structure CreateOrderRequest {
    @required
    product: String

    @required
    @range(min: 1)
    quantity: Integer
}

structure Order {
    @required
    id: String

    @required
    product: String

    @required
    quantity: Integer
}

structure CreateOrderInput {
    @httpPayload
    @required
    @contentType("application/json")
    body: CreateOrderRequest
}

structure CreateOrderOutput {
    @httpPayload
    @required
    @contentType("application/json")
    body: Order
}

@http(method: "POST", uri: "/orders", code: 201)
@responseRelations({
    "201": {
        "application/json": {
            bindings: [
                { from: "/product", to: "/product" }
                { from: "/quantity", to: "/quantity" }
            ]
        }
    }
})
operation createOrder {
    input: CreateOrderInput
    output: CreateOrderOutput
}

service Orders {
    version: "1.0.0"
    operations: [createOrder]
}
```

### Semantic TypeScript

Create `orders.d.ts`:

```ts
export type ServiceName = "Orders";
export type CollectionVersion = "1.0.0";
export type Closed<Value> = Value;

export type CreateOrderRequest = Closed<{
  readonly product: string;
  readonly quantity: number;
}>;

export type Order = Closed<{
  readonly id: `or_${string}`;
  readonly product: string;
  readonly quantity: number;
}>;

export interface OrdersOperations {
  readonly createOrder: {
    readonly method: "POST";
    readonly path: "/orders";
    readonly request: {
      readonly required: true;
      readonly content: {
        readonly "application/json": CreateOrderRequest;
      };
    };
    readonly responses: {
      readonly "201": {
        readonly content: {
          readonly "application/json": Order;
        };
      };
    };
  };
}

export interface ResponseRelations {
  readonly createOrder: {
    readonly "201": {
      readonly "application/json": {
        readonly bindings: readonly [
          { readonly from: "/product"; readonly to: "/product" },
          { readonly from: "/quantity"; readonly to: "/quantity" },
        ];
      };
    };
  };
}
```

Each contract gives Simile enough information to validate a request and build a valid response.
There is no HCL file yet because this service has no additional public behavior to declare.

## 2. Add the service composition

Create `Simfile` next to the selected contract. Use exactly one of these source blocks:

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

```hcl
service "orders" {
  listen {
    host = "127.0.0.1"
    port = 4242
  }
  smithy { path = "./orders.smithy" }
}
```

```hcl
service "orders" {
  listen {
    host = "127.0.0.1"
    port = 4242
  }
  typescript { path = "./orders.d.ts" }
}
```

The configuration answers operational questions—what the service is called, where it listens,
and where its contract comes from. Routes and schemas remain in the selected contract.

## 3. Register and start it

```bash
simile register
simile start orders
```

Registration compiles the source and atomically installs a validated `.similec` artifact under
`~/.simile`. Starting by name loads that artifact; it does not reopen the contract source.

A service that declares `principal_setup` adds one command between registration and start:

```bash
simile register inttegro --config ./inttegro.simile.hcl
simile setup inttegro
simile start inttegro
```

The profile keeps that application's credentials and webhook configuration together while its
principal owns the shared credential grants.
See [Service profiles](/service-profiles) for the model and [Setup](/service-setup) for the complete
workflow.

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

The exact generated `id` varies. `product` and `quantity` are projected from the validated request.
OpenAPI can use compatible same-path projection; the Smithy and TypeScript examples declare those
bindings explicitly. The complete response is validated again before it is returned.

## 4. Run an operation without a server

`simile run` is useful for authoring and CI checks:

```bash
simile run ./orders.yaml --operation-id createOrder
simile run ./orders.smithy --operation-id createOrder
simile run ./orders.d.ts --operation-id createOrder
```

Run only the command for the contract you selected.

Because no request was provided, Simile synthesizes one, validates it, executes the operation, and
prints only the pretty JSON response. Pass `--verbose` to see the normalized request, response
metadata, and outcome provenance.

You can also provide a full request document:

```bash
simile run ./orders.yaml \
  --operation-id createOrder \
  --request '{"headers":{"Content-Type":"application/json"},"body":{"product":"jingle","quantity":1}}'
```

Replace `./orders.yaml` with the Smithy or TypeScript path when that is the contract you selected.
The same `--request` option accepts a file path or `-` for standard input.

## 5. Update the registered service

After editing the selected contract, compile a new immutable revision and move the registration to
it:

```bash
simile update orders
```

If compilation fails, the current revision remains active. Restart the service process to load the
new artifact.

## 6. Run an application test

When an application test suite should own the emulator's lifetime, run it through Simile:

```bash
simile test orders -- npm test
```

The child command receives `SIMILE_BASE_URL`. Simile binds before starting the tests, selects an
available port, returns the test command's exit status, and shuts down afterward. Continue with
[Local testing](/local-testing) or [GitHub Actions](/github-actions).

Next, read the authoring standard for [OpenAPI](/openapi-and-hcl),
[Smithy](/smithy-contracts), or [semantic TypeScript](/typescript-contracts). Use
[Contract sources and HCL](/contract-sources-and-hcl) to add named outcomes and effects, and
[Service lifecycle](/service-lifecycle) to understand what registration stores.
