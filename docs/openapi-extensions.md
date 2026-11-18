---
title: Overview
description: An overview of the executable contract vocabulary Simile adds to OpenAPI.
---

OpenAPI describes individual fields well, but many valid service payloads depend on relationships
between fields. Simile adds `x-simile-constraints` for relationships inside one payload and
`x-simile-from-request` for public relationships between an operation's request and response.

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
| `x-simile-from-request` | Preserved inputs, derived response values, reachable states, and ineligible optional data | [`x-simile-from-request`](/request-response-relations) |
| `x-simile-constraints` | Executable relationships among values inside one payload | [`x-simile-constraints`](/x-simile-constraints) |
| Mechanics | Placement, JSON Pointers, selectors, predicates, guards, and authoring rules | [Constraint mechanics](/constraint-mechanics) |
| Value relationships | Ordering, bounds, equality, distinct values, and scalar-set relationships | [Value relationships](/constraint-value-relationships) |
| Conditional and derived values | Implications, arithmetic equations, and collection aggregates | [Conditional and derived values](/constraint-conditional-derived) |
| Stateful and rendered values | Finite lifecycle paths and strings rendered from fields | [Lifecycles and templates](/constraint-lifecycle-template) |

<Note>
Use ordinary OpenAPI keywords for field-local rules such as `required`, `enum`, `minimum`,
`maxLength`, `oneOf`, and `format`. Use `x-simile-constraints` only when multiple values must be
understood together.
</Note>

For the complete constraint vocabulary, read [`x-simile-constraints`](/x-simile-constraints).
For the design model and proof semantics, read [Payload soundness](/payload-soundness).
For approximate generation when a provider has not published a relationship, read
[Generation heuristics](/generation-heuristics).
