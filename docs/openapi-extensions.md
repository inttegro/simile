---
title: OpenAPI extensions
description: An overview of the executable contract vocabulary Simile adds to OpenAPI.
---

OpenAPI describes individual fields well, but many valid service payloads depend on relationships
between fields. Simile adds `x-simile-constraints` for those relationships.

For example, OpenAPI can require `created_at` and `updated_at` to be RFC 3339 timestamps. The Simile
extension can additionally require them to appear in chronological order:

```yaml
type: object
required: [created_at, updated_at]
properties:
  created_at: { type: string, format: date-time }
  updated_at: { type: string, format: date-time }
x-simile-constraints:
  - ordered:
      paths: [/created_at, /updated_at]
```

The result is executable contract rather than descriptive metadata. Simile compiles each clause for
two jobs:

- **proof** — reject an incoming request or generated response that violates the relationship;
- **construction** — establish the relationship while synthesizing a request or response.

Invalid paths, incompatible types, contradictory clauses, and unsupported operators fail service
compilation. Simile never silently ignores a constraint it cannot execute.

## Extension map

| Topic | What it covers | Reference |
| --- | --- | --- |
| Mechanics | Placement, JSON Pointers, selectors, predicates, guards, and authoring rules | [Constraint mechanics](/constraint-mechanics) |
| Value relationships | Ordering, bounds, equality, distinct values, and scalar-set relationships | [Value relationships](/constraint-value-relationships) |
| Conditional and derived values | Implications, arithmetic equations, and collection aggregates | [Conditional and derived values](/constraint-conditional-derived) |
| Stateful and rendered values | Finite lifecycle paths and strings rendered from fields | [Lifecycles and templates](/constraint-lifecycle-template) |

## Clause glossary

| Clause | What it expresses | Typical use |
| --- | --- | --- |
| `ordered` | Present values appear in nondecreasing order | Created, published, and updated timestamps |
| `bounded` | One value stays within bounds supplied by other fields | Paid amount between refunded and due amounts |
| `implies` | When a predicate is true, all declared consequences must hold | An inactive product has no publication timestamp |
| `equal` | Every selected scalar has the same value | Invoice and line-item currencies agree |
| `distinct` | Every selected scalar is unique | No duplicate line-item IDs |
| `set` | Two scalar arrays have a subset, superset, or disjoint relationship | Enabled methods are supported methods |
| `equation` | A numeric target equals a finite arithmetic expression | `remaining = due - paid` |
| `aggregate` | A numeric target equals a collection sum, count, minimum, or maximum | Invoice subtotal equals the sum of line amounts |
| `lifecycle` | A state and its timestamps form a valid finite transition path | Draft, open, paid, void, and uncollectible snapshots |
| `template` | A string equals a template rendered from required scalar fields | A collection URL contains its parent resource ID |

<Note>
Use ordinary OpenAPI keywords for field-local rules such as `required`, `enum`, `minimum`,
`maxLength`, `oneOf`, and `format`. Use `x-simile-constraints` only when multiple values must be
understood together.
</Note>

For the design model and complete proof semantics, read [Payload soundness](/payload-soundness).
For approximate generation when a provider has not published a relationship, read
[Generation heuristics](/generation-heuristics).
