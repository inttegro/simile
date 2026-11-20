---
title: x-simile-from-request
description: Prove that a synthesized response is reachable from the request that produced it.
---

OpenAPI normally validates a request and a response as two independent documents. That proves each
payload is structurally legal, but it cannot prove that the response could result from the submitted
request. Simile closes that gap with `x-simile-from-request` on a response media type.

Use the extension only for public API behavior: values the service promises to preserve, totals it
promises to derive, lifecycle outcomes controlled by public request fields, and fields that cannot
appear for a given public flow. It should not describe private risk checks or implementation details.

## A complete relation

```yaml
responses:
  "200":
    description: Order created
    content:
      application/json:
        schema:
          $ref: "#/components/schemas/OrderResponse"
        x-simile-from-request:
          bindings:
            - from: /customer_id
              to: /order/customer/id
            - from: /number
              to: /order/number
            - from: /line_items
              to: /order/line_item_group/line_items
            - from: /line_items/*/product/price/currency
              to: /order/line_item_group/total/currency
            - to: /order/line_item_group/total/value
              value:
                sum:
                  each: /line_items/*
                  value:
                    multiply:
                      - { path: /product/quantity }
                      - { path: /product/price/value }
          rules:
            - when:
                all:
                  - { request: /execute_payment, equals: false, default: false }
                  - { request: /finalize, equals: true, default: false }
              then:
                set:
                  /order/status: requires_payment
                omit:
                  - /order/paid_at
                  - /order/refunds
```

This declaration proves five different facts about the result:

- the customer and order number are the values supplied by the caller;
- line items preserve caller-owned fields while response-only fields such as IDs are synthesized;
- the total equals the submitted unit prices multiplied by their quantities;
- omitting `execute_payment` uses its public default of `false`;
- the resulting lifecycle is `requires_payment`, with paid-only and refund data absent.

## Bindings

A binding with `from` copies a validated request value into a response target. A plain string is an
absolute JSON Pointer into the request body. Body selectors may contain `*` to traverse arrays. The
response target must name one concrete property.

```yaml
- from: /line_items
  to: /order/line_item_group/line_items
```

Projection is schema-aware rather than a raw JSON copy. Simile retains response-only fields needed
by the response schema, rejects incompatible values, and proves after construction that the target
still contains the submitted request value. A binding can declare `default` when omission has a
publicly specified meaning.

### Request locations

Relations can also read path, query, header, and cookie parameters. Parameter sources name the
OpenAPI parameter rather than a JSON Pointer:

```yaml
x-simile-from-request:
  bindings:
    - from: { path: payout_id }
      to: /payout/id
    - from: { query: page_number }
      to: /page/number
    - from: { header: X-Trace-Id }
      to: /trace_id
    - from: { cookie: session }
      to: /session
```

Header names are matched case-insensitively. An explicit body source can use
`from: { body: /customer_id }`; it is equivalent to `from: /customer_id`. Simile compiles each
source against the request body or parameter schema and reads only the value returned by request
validation. A relation can combine body and parameter sources, including a multipart body with a
query or header parameter.

## Derived values

A binding with `value` computes its target. Supported expressions are:

- `request` for an absolute request selector;
- `path` for a selector relative to the current item inside `sum`;
- `literal` for a fixed JSON value;
- `add` and `multiply` for finite numeric expressions; and
- `sum` for a value derived from every item selected by `each`;
- `concat` for joining string expressions;
- `substring` for selecting a Unicode character range, with negative indexes counted from the
  end;
- `mask` for replacing all but the last `reveal` characters; and
- `coalesce` for selecting the first expression that resolves to a value.

For example, a public tokenization contract can derive the fields that callers use for display:

```yaml
bindings:
  - to: /last_four
    value:
      substring:
        value: { request: /account_number }
        start: -4
  - to: /masked
    value:
      mask:
        value: { request: /account_number }
        reveal: 4
        character: "*"
  - to: /label
    value:
      concat:
        - { literal: "acct-" }
        - { substring: { value: { request: /account_number }, start: -4 } }
```

Integer inputs remain integer-shaped when the result fits the JSON integer range. Compilation
rejects unresolved selectors, malformed expressions, and response targets not declared by the
response schema.

## Guarded rules

Rules make public request controls select reachable response states. Every condition in `when.all`
must match. A condition may declare a `default` for an omitted request field.

`then.set` assigns response values accepted by their schemas. `then.omit` removes optional data that
cannot exist in the selected flow. Compilation rejects attempts to omit required response fields.

## Proof levels

Every emitted response is validated against its standalone response schema. When validated request
values and a compiled relation are both present, Simile additionally applies and proves the
relation, and reports the response as `RequestConstrained`. The proof uses the request body and any
compiled path, query, header, or cookie sources referenced by the relation. Without that relation,
the result is reported as `SchemaOnly` even if same-path structural projection improved it.

That distinction is deliberate: Simile does not claim an operationally reachable result unless it
has executable public proof material for the relationship.

Read [Response synthesis](/response-synthesis) for runtime selection and construction, or
[Payload soundness](/payload-soundness) for relationships within one payload.
