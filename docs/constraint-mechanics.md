---
title: Constraint mechanics
description: Where constraints live and how paths, selectors, predicates, and guards behave.
---

Add `x-simile-constraints` to the object schema that owns the related fields. Its value is an array,
and every array item contains exactly one clause:

```yaml
Order:
  type: object
  properties:
    # ...
  x-simile-constraints:
    - ordered: { ... }
    - equation: { ... }
```

Constraints on an array's `items` object apply independently to each validated or generated item.
Constraints are inherited when a schema is reached through `$ref`; do not copy them onto every
operation that uses the schema.

## JSON Pointer

Every `path` is an RFC 6901 JSON Pointer relative to the object containing the extension.
`/status` selects the `status` property, and `/status_transitions/paid_at` selects a nested property.

Escape `~` as `~0` and `/` inside a property name as `~1`.

```yaml
- ordered:
    paths:
      - /created_at
      - /status_transitions/paid_at
```

The compiler resolves each path against the schema. A missing property, incompatible value kind,
or path that crosses an unsupported structure is a contract error.

## Selector

A selector is a JSON Pointer that may use a complete `*` segment to visit every item in an array.
For example, `/lines/data/*/amount` selects every line amount.

```yaml
- distinct:
    source: /lines/data/*/id
```

Wildcards are available only to clauses designed for collections. They do not mean arbitrary
property matching and cannot appear inside a property name.

## Field predicate

A field predicate has a `path` and one assertion:

- `equals` — exact JSON scalar or structural equality;
- `contains` — an array contains the given value;
- `state` — `present`, `absent`, `null`, `non-null`, or `null-or-absent`.

```yaml
{ path: /published_at, state: null-or-absent }
```

An explicitly null property is present. An absent property has no member in the containing object.
This distinction lets a public contract express omission separately from nullable data.

## Guard

A guard decides whether a relationship applies. It appears as the `when` value of an aggregate or
as the antecedent of an implication.

```yaml
when: { path: /lines/has_more, equals: false }
```

A false guard makes the relationship vacuously sound. It does not mutate the payload or imply the
opposite relationship.

## Authoring rules

- Publish only relationships that are part of the service's public contract.
- Keep private risk checks, database decisions, vendor rules, and production thresholds out of
  distributed OpenAPI.
- Prefer ordinary OpenAPI whenever it can express the rule without an extension.
- Put a relationship on the narrowest object schema that owns all participating fields.
- Treat compilation errors as contract errors; Simile never ignores a clause it cannot prove and
  construct.
- Remember that constraints prove published behavior, not undisclosed production invariants.

Continue to [Value relationships](/constraint-value-relationships) for ordering, bounds, equality,
uniqueness, and set clauses.
