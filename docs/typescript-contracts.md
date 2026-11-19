---
title: Semantic TypeScript authoring standard
sidebarTitle: TypeScript
description: Author a declarations-only HTTP contract with semantic payload types, soundness propositions, examples, and request-to-response relationships.
---

Semantic TypeScript is a first-class Simile contract source. A contract is one or more `.d.ts`
files that describe payload values and HTTP operations directly. Simile parses the declarations
with OXC; it never executes JavaScript, invokes `tsc`, or expects an object representation of
another contract format.

## Minimal runnable service

Create `orders.d.ts`:

```ts
export type ServiceName = "Orders";
export type CollectionVersion = "1.0.0";

export type Closed<Value> = Value;
export type DateTime = string;
export type FieldPath = readonly string[];
export interface Ordered<Owner, Paths extends readonly FieldPath[]> {
  readonly relation: "ordered";
  readonly owner?: Owner;
  readonly paths: Paths;
}

export type CreateOrderRequest = Closed<{
  readonly product: string;
  readonly quantity: number;
}>;

export type Order = Closed<{
  readonly id: `or_${string}`;
  readonly product: string;
  readonly quantity: number;
  readonly created_at: DateTime;
  readonly updated_at: DateTime;
}>;

export interface SchemaSoundness {
  readonly Order: readonly [
    Ordered<Order, readonly [
      readonly ["created_at"],
      readonly ["updated_at"],
    ]>,
  ];
}

export interface OrdersOperations {
  readonly createOrder: {
    readonly method: "POST";
    readonly path: "/orders";
    readonly tags: readonly ["Orders"];
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

Compile and execute it:

```bash
simile compile ./orders.d.ts --artifact ./orders.similec
simile run ./orders.d.ts --operation-id createOrder
```

## Operation registry

Simile discovers public operation interfaces whose names end in `Operations`. Every member name is
an `operationId`; every member declares a literal `method`, literal `path`, optional `tags` and
`parameters`, an optional request, and declared responses.

```ts
export interface OrdersOperations {
  readonly getOrder: {
    readonly method: "GET";
    readonly path: "/orders/{order_id}";
    readonly parameters: {
      readonly path: { readonly order_id: `or_${string}` };
      readonly query: { readonly expand?: string };
      readonly header: { readonly "X-Request-Id"?: string };
      readonly cookie: Record<string, never>;
    };
    readonly responses: {
      readonly "200": {
        readonly content: { readonly "application/json": Order };
      };
    };
  };
}
```

Path parameters are always required. Optional properties control requiredness for query, header,
and cookie parameters. Request and response `content` keys are literal media types. Use a numeric
status string such as `"201"` or a supported status range declared by the contract package.

## Payload types

Use declarations that describe public JSON values: objects, interfaces, literal types, unions,
intersections, tuples, arrays, template literal types, and references to other payload types.
Simile recognizes `DateTime`, `Uri`, `EmailAddress`, and `Binary` as formatted scalar aliases.

The current frontend lowers TypeScript's `number` to a JSON number. It does not infer integer,
numeric-bound, string-length, or array-length facets from a type name. Use literal unions and
template literal types when they state the real finite or patterned contract; otherwise report the
unsupported local facet instead of implying that a branded alias enforces it.

TypeScript object types are structurally open. Wrap an object in `Closed<T>` when the service
rejects undeclared properties. Optional properties remain distinct from required properties whose
value includes `null`.

Request and response direction can use separate payload types. A split provider contract may also
publish `Input` and `Output` namespaces, but the operation registry remains the authority for which
type is used on each wire edge.

## Soundness propositions

`SchemaSoundness` attaches public cross-field propositions to their owning payload type. Simile
lowers these semantic types into its shared schema constraints and uses them for validation and
synthesis.

Supported relationship types are:

- `Ordered`, `Bounded`, `Implies`, and `EqualFields`;
- `DistinctValues` and `SetRelationship`;
- `Equation` with `Field`, `Literal`, `Add`, `Subtract`, and `Multiply` expressions;
- `Aggregate` and `Template`; and
- `Lifecycle`.

Predicate types are `EqualsAt`, `StateAt`, `ContainsAt`, and `CompareAt`. Paths are tuples of field
segments; use the entire `"*"` segment to select every array item. Attach each proposition to the
narrowest payload owner that contains all participating fields.

Use ordinary TypeScript types for the field-local rules they can express. Do not create a semantic
proposition merely to restate requiredness, literal values, or nullability, and do not use a
cross-field relationship to approximate an unsupported scalar facet.

## Request-to-response relationships

Publish `ResponseRelations` or resource-local interfaces ending in `ResponseRelations`. The
structure is operation ID, response status, content type, then the literal relation document:

```ts
export interface OrdersResponseRelations {
  readonly createOrder: {
    readonly "201": {
      readonly "application/json": {
        readonly bindings: readonly [
          { readonly from: "/reference"; readonly to: "/order/reference" },
        ];
      };
    };
  };
}
```

Bindings, rules, derived values, omissions, and defaults must use public request and response paths.
Simile checks the relation against the operation's declared payloads before it can construct a
response.

## Provider-authored examples

Literal witnesses live in `OperationExamples`. Request examples are keyed by name; response
examples are additionally grouped by status:

```ts
export interface OperationExamples {
  readonly createOrder: {
    readonly request: {
      readonly one_item: {
        readonly product: "beat_pack";
        readonly quantity: 1;
      };
    };
    readonly responses: {
      readonly "201": {
        readonly created: {
          readonly id: "or_example";
          readonly product: "beat_pack";
          readonly quantity: 1;
          readonly created_at: "2026-11-19T12:00:00Z";
          readonly updated_at: "2026-11-19T12:00:00Z";
        };
      };
    };
  };
}
```

Keep witness properties literal so Simile can retain their exact values. Every example must
validate against the corresponding request or response payload.

## Split declaration packages

A `.d.ts` entrypoint recursively loads only relative declaration imports and exports. Bare package
imports are not fetched. Passing a directory loads every non-test `.d.ts` file below it. A remote
TypeScript URL must therefore contain the whole contract in one document.

A practical package separates `core`, payload models, request and response types, operation groups,
soundness propositions, response relations, and examples behind a small `commerce.d.ts` barrel.
Unsupported declarations that are not referenced by the public payload graph are ignored;
unsupported referenced declarations fail compilation.

Use the package in a service configuration with:

```hcl
service "orders" {
  listen { port = 4242 }
  typescript { path = "./typescript/commerce.d.ts" }
}
```

Next, read [Contract sources and HCL](/contract-sources-and-hcl) for source ownership or
[Payload soundness](/payload-soundness) for the shared proof and synthesis model.
