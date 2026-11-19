---
title: Smithy authoring standard
sidebarTitle: Smithy
description: Author a standalone Smithy 2.0 HTTP contract with payload soundness, examples, errors, and request-to-response relationships.
---

Smithy 2.0 is a first-class Simile contract source. The model is standalone: Simile derives the
HTTP surface, payload schemas, examples, errors, soundness rules, and response construction from
the Smithy document without consulting OpenAPI or another contract format.

## Minimal runnable service

Create `orders.smithy`:

```smithy
$version: "2.0"
namespace example.orders

@trait(selector: "member")
string contentType

@trait(selector: "*")
document simileConstraints

@trait(selector: "operation")
document responseRelations

structure CreateOrderRequest {
    @required
    product: String

    @required
    @range(min: 1)
    quantity: Integer
}

@simileConstraints([
    {
        ordered: {
            paths: ["/created_at", "/updated_at"]
        }
    }
])
structure Order {
    @required
    id: String

    @required
    product: String

    @required
    quantity: Integer

    @required
    created_at: Timestamp

    @required
    updated_at: Timestamp
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
@tags(["Orders"])
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

Compile and execute it:

```bash
simile compile ./orders.smithy --artifact ./orders.similec
simile run ./orders.smithy --operation-id createOrder
```

## HTTP operations and payloads

Every operation Simile executes needs `@http`. The operation name becomes its stable
`operationId`. Input members use Smithy's HTTP binding traits for parameters:

| Trait | HTTP location |
| --- | --- |
| `@httpLabel` | Path parameter; always required. |
| `@httpQuery("name")` | Query parameter. |
| `@httpHeader("Name")` | Header parameter. |
| `@httpCookie("name")` | Cookie parameter. |
| `@httpPayload` | Request or response body. |

Simile currently reads a body from the member marked `@httpPayload`. Add `@required` when the body
must be present and use the local `@contentType` trait when it is not `application/json`.

```smithy
structure GetOrderInput {
    @httpLabel
    @required
    order_id: String

    @httpQuery("expand")
    expand: String

    @httpHeader("X-Request-Id")
    request_id: String
}
```

## Errors and exact outcomes

Attach errors to the operation and give every error shape both `@error` and `@httpError`:

```smithy
@error("client")
@httpError(400)
structure InvalidOrder {
    @httpPayload
    @required
    @contentType("application/json")
    body: ErrorBody
}

structure ErrorBody {
    @required
    code: String

    @required
    message: String
}
```

Use the local `dataExamples` trait on a payload member or shape to publish exact JSON witnesses.
Simile retains those examples as provider-authored outcomes when they validate against the shape.

## Public semantic traits

Declare Simile's traits in the model that uses them. That keeps the published Smithy source
self-contained and makes the semantic surface reviewable by Smithy consumers.

| Trait | Target | Meaning |
| --- | --- | --- |
| `simileConstraints` | Shape or member | Cross-field relationships compiled into the schema graph. |
| `responseRelations` | Operation | Request-to-response bindings, rules, and derived values by status and content type. |
| `simileSchema` | Shape or member | A schema refinement the base Smithy shape algebra cannot state. |
| `httpSuccessResponses` | Structure | Additional successful HTTP response alternatives. |
| `dataExamples` | Shape or member | Provider-authored JSON examples. |
| `contentType` | Member | Media type for an HTTP payload member. |
| `untagged` | Union | Emit the union alternatives without Smithy's member-name wrapper. |
| `collectionVersion` | Service | Published contract collection version. |
| `availability` | Any shape | Public availability metadata retained with the model. |

`simileConstraints` uses the same public relationship vocabulary as Simile's schema engine:
ordering, dynamic bounds, implications, equality, distinctness, set relationships, equations,
aggregates, templates, and lifecycle snapshots. Every referenced path is relative to the shape
carrying the trait and must resolve to a compatible member.

Use ordinary Smithy traits such as `@required`, `@pattern`, `@length`, `@range`, `@uniqueItems`,
`@readonly`, and `@documentation` for single-field rules. Reserve `simileConstraints` for
relationships involving more than one value.

## Mixins, unions, and closure

Simile flattens Smithy structure mixins using member-copy semantics. Structures compile as closed
JSON objects. Tagged unions retain Smithy's member-name wrapper; apply the local `@untagged` trait
when the public JSON uses unwrapped alternatives. A `simileSchema` discriminator can materialize
the discriminator member on its closed target shapes.

## Current source boundary

Simile accepts one standalone `.smithy` document per compilation. It does not currently assemble a
multi-file Smithy model or resolve external model packages. Prelude shapes and traits are
available, and every custom Simile trait must be declared locally. Compilation fails on unresolved
operations, invalid HTTP statuses, incompatible semantic paths, and malformed trait values.

Use this contract in a service configuration with:

```hcl
service "orders" {
  listen { port = 4242 }
  smithy { path = "./orders.smithy" }
}
```

Next, read [Contract sources and HCL](/contract-sources-and-hcl) for source ownership or
[Payload soundness](/payload-soundness) for the shared proof and synthesis model.
