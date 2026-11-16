# Payload soundness

Simile treats a request or response payload as sound only when it satisfies the complete compiled
contract. The contract includes the supported OpenAPI schema rules and every recognized
cross-field relationship declared with `x-simile-constraints`.

There is one authority for relationships: `simile-schema::soundness`. It owns extension parsing,
compiled constraint types, proof evaluation, and the establishment step used during synthesis. The
schema validator delegates all multi-value rules to that module. Incoming request payloads are
accepted only after the complete validator reports no violations. Request and response synthesis
use the same validator as their final gate, so generation cannot bypass a relationship that
validation enforces. The compiled `.similec` artifact carries the typed constraints; the runtime
does not reparse source OpenAPI or infer relationships from property names or prose.

This is a proof relative to the published contract. It does not claim that an incomplete contract
describes every private production invariant. A service provider must publish each relationship it
wants Simile to prove.

## Ordered fields

Ordering is the first supported relationship:

```yaml
Product:
  type: object
  required: [created_at, updated_at]
  properties:
    created_at:
      type: string
      format: date-time
    published_at:
      type: [string, "null"]
      format: date-time
    updated_at:
      type: string
      format: date-time
  x-simile-constraints:
    - ordered:
        paths:
          - /created_at
          - /published_at
          - /updated_at
```

The paths are RFC 6901 JSON Pointers relative to the object that owns the extension. An ordered
constraint means each present, non-null value is less than or equal to the next present, non-null
value. Equality is valid. Ordinary OpenAPI `required` and nullable declarations continue to decide
whether a value may be absent or null.

The compiler accepts an ordered relationship only when:

- it contains at least two unique paths;
- every path resolves to a declared property;
- all paths resolve to compatible numeric values, or to strings explicitly formatted as
  `date-time`; and
- the relationship can be represented by the compiled schema graph.

An unknown path, malformed JSON Pointer, unsupported value kind, or mixed comparison kind fails
compilation. Simile does not retain an unprovable relationship as inert metadata.

Validation reports an `ordered` violation at the first later field that breaks the relationship.
Seeded synthesis generates values and then establishes the relationship before sending the complete
candidate through the same validator. If no sound finite candidate can be found within the bounded
search, synthesis fails instead of returning an unsound payload.

## Extending relationships

The compiled constraint model is a typed enum, not timestamp-specific code. Later relationships
will be added as new compiler-validated variants and executed by the same validator and synthesis
pipeline. For example, the product invariant “an inactive product cannot have a publication time”
requires a conditional relationship between `active` and `published_at`; ordering alone cannot
express it, and Simile does not pretend otherwise. Conditional relationships are not accepted yet.
