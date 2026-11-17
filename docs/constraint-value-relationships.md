---
title: Value relationships
description: Ordering, bounds, equality, uniqueness, and relationships between scalar arrays.
---

These clauses compare already selected values. Ordinary OpenAPI still controls whether each field
is required, nullable, formatted, or locally bounded.

## `ordered`

Use `ordered` for compatible numbers or `string` fields with `format: date-time`. Present, non-null
values must appear in nondecreasing order. Missing and null values are skipped; OpenAPI decides
whether they are allowed.

```yaml
- ordered:
    paths:
      - /created_at
      - /published_at
      - /updated_at
```

## `bounded`

Use `bounded` when one constructive target is limited by required, non-null source fields.
Supported keys are `minimum`, `maximum`, `exclusive-minimum`, and `exclusive-maximum`.

```yaml
- bounded:
    path: /amount_paid
    minimum: { path: /amount_refunded }
    maximum: { path: /amount_due }
```

The compiler requires compatible numeric value kinds and enough information to construct the target
inside the declared interval.

## `equal`

Use `equal` when compatible scalar values selected from fields or array items must agree.

```yaml
- equal:
    paths:
      - /currency
      - /lines/data/*/currency
```

Every present selected value participates. OpenAPI determines whether missing or null values are
otherwise allowed.

## `distinct`

Use `distinct` when a selector must not produce duplicate scalar values. The selector must cross at
least one array wildcard.

```yaml
- distinct:
    source: /lines/data/*/id
```

## `set`

Use `set` for relationships between arrays of compatible scalar values. The right-hand array is the
required reference; the left-hand array is the value Simile may construct.

```yaml
- set:
    left: /enabled_methods
    relation: subset-of
    right: /supported_methods
```

Supported relations are:

- `subset-of` — every left value appears on the right;
- `superset-of` — every right value appears on the left;
- `disjoint` — no value appears in both arrays.

For rules that activate conditionally or derive numeric values, continue to
[Conditional and derived values](/constraint-conditional-derived).
