---
title: Conditional and derived values
description: Implications, arithmetic equations, and aggregates over collections.
---

These clauses express relationships whose applicability or result depends on other values in the
same object.

## `implies`

Use `implies` for conditional presence, nullability, or value rules. When `when` is false, the
constraint is satisfied. When it is true, every predicate in `then.all` must hold.

```yaml
- implies:
  when: { path: /active, equals: false }
  then:
    all:
      - { path: /published_at, state: null-or-absent }
      - { path: /checkout_enabled, equals: false }
```

One antecedent can therefore require several consequences without repeating the same `when` block.

The antecedent may also compare two compatible numeric or date-time fields:

```yaml
- implies:
  when:
    compare:
      left: { path: /amount_paid }
      operator: greater-than
      right: { path: /amount_due }
  then:
    all:
      - { path: /overpaid, equals: true }
```

## `equation`

Use `equation` to derive one numeric target from required, non-null numeric sources.

```yaml
- equation:
  target: /amount_remaining
  value:
    subtract:
      - { path: /amount_due }
      - { path: /amount_paid }
```

Expression leaves are `{ path: /field }` or `{ literal: 200 }`. `add` and `multiply` accept two or
more expressions; `subtract` accepts exactly two. Equations do not infer rounding, units, or
currency semantics.

## `aggregate`

Use `aggregate` to relate a numeric target to values selected from an array.

```yaml
- aggregate:
  target: /subtotal
  operation: sum
  source: /lines/data/*/amount
  when: { path: /lines/has_more, equals: false }
```

Operations are `sum`, `count`, `min`, and `max`. An optional `when` guard is useful when a partial
page cannot prove an aggregate over the complete collection.

For state transitions and strings rendered from fields, continue to
[Lifecycles and templates](/constraint-lifecycle-template).
