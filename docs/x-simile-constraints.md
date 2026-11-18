---
title: x-simile-constraints
description: Express and prove relationships among values inside one request or response payload.
---

`x-simile-constraints` describes relationships that ordinary OpenAPI keywords cannot express
inside a single payload. It turns cross-field invariants into executable contract: Simile uses the
same declaration both to construct sound payloads and to reject payloads that violate the declared
relationship.

Place the extension on the object schema whose values participate in the relationship:

```yaml
type: object
required: [created_at, published_at, updated_at]
properties:
  created_at: { type: string, format: date-time }
  published_at: { type: string, format: date-time }
  updated_at: { type: string, format: date-time }
x-simile-constraints:
  - ordered:
      paths: [/created_at, /published_at, /updated_at]
```

This contract proves that the timestamps are individually valid and appear in chronological order.
Simile also establishes that order when it synthesizes the object.

## What belongs here

Use `x-simile-constraints` for public relationships among two or more values in the same document:

- ordering and relative bounds;
- equality, uniqueness, and scalar-set relationships;
- conditional presence or values;
- arithmetic equations and collection aggregates;
- lifecycle states and their associated timestamps; and
- strings derived from other public fields.

Use ordinary OpenAPI keywords for field-local rules such as `required`, `enum`, `minimum`,
`maxLength`, `oneOf`, and `format`. Use
[`x-simile-from-request`](/request-response-relations) when the relationship crosses from an
operation request into its response.

## Clause families

| Clause | Relationship | Typical use |
| --- | --- | --- |
| `ordered` | Present values are nondecreasing | Created, published, and updated timestamps |
| `bounded` | A value remains within bounds supplied by other fields | Refunded amount does not exceed the paid amount |
| `equal` | Selected scalar values are equal | Invoice and line-item currencies agree |
| `distinct` | Selected scalar values are unique | No duplicate line-item IDs |
| `set` | Scalar arrays are subsets, supersets, or disjoint | Enabled methods are supported methods |
| `implies` | A predicate requires one or more consequences | An inactive product has no publication timestamp |
| `equation` | A numeric target equals an expression | `remaining = due - paid` |
| `aggregate` | A numeric target summarizes a collection | A subtotal equals the sum of line amounts |
| `lifecycle` | State and transition fields form a reachable snapshot | Draft, open, paid, void, or uncollectible invoices |
| `template` | A string is rendered from required scalar fields | A collection URL contains its parent resource ID |

## Compilation and proof

Simile resolves every selector against the schema while compiling the service. Invalid paths,
incompatible types, contradictory clauses, and unsupported operators fail compilation instead of
being ignored.

At runtime, the compiled clauses have two responsibilities:

1. **Construction** establishes the declared relationships during request or response synthesis.
2. **Proof** validates the completed payload after every construction and repair step.

This symmetry matters: an annotation that can only describe an invariant, but cannot participate in
construction and proof, is not an executable Simile constraint.

## Continue into the grammar

- [Constraint mechanics](/constraint-mechanics) covers placement, JSON Pointers, selectors, and authoring rules.
- [Value relationships](/constraint-value-relationships) covers ordering, bounds, equality, uniqueness, and sets.
- [Conditional and derived values](/constraint-conditional-derived) covers implications, equations, and aggregates.
- [Lifecycles and templates](/constraint-lifecycle-template) covers state transitions and rendered strings.

For the end-to-end proof model, read [Payload soundness](/payload-soundness).
