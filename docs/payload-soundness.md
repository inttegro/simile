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

Paths are RFC 6901 JSON Pointers relative to the object that owns the extension. Selectors used by
collection relationships additionally reserve a complete `*` segment to visit every item in an
array, as in `/lines/data/*/amount`. An ordered
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

## Conditional implications

An implication lets one field state constrain several others:

```yaml
Product:
  type: object
  required: [active, published_at, archived_at, checkout_enabled]
  properties:
    active: { type: boolean }
    published_at: { type: [string, "null"], format: date-time }
    archived_at: { type: [string, "null"], format: date-time }
    checkout_enabled: { type: boolean }
  x-simile-constraints:
    - implies:
        when:
          path: /active
          equals: false
        then:
          all:
            - { path: /published_at, state: null-or-absent }
            - { path: /archived_at, state: non-null }
            - { path: /checkout_enabled, equals: false }
```

The `when` predicate is the antecedent. When it is false, the implication is satisfied without
examining its consequences. When it is true, every predicate under `then.all` must hold. Simile
accepts `equals`, `contains`, and `state` field predicates. `contains` applies to arrays. State is
one of `present`, `absent`, `null`, `non-null`, or `null-or-absent`; an explicitly null property is
present, while an absent property has no value in the object.

The antecedent can instead compare two numeric or RFC 3339 date-time fields:

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

Comparison operators are `greater-than`, `greater-than-or-equal`, `less-than`, and
`less-than-or-equal`. A comparison is an antecedent, not an assignment expression. Consequences
remain constructive field assertions so Simile can compile a schema-valid witness for each one.

Every path must resolve to a declared property, every predicate must have a witness allowed by the
property schema, and assertions for the same path must be jointly satisfiable. Unsupported branches
such as `then.any`, empty consequence sets, dead predicates, and contradictory consequences fail
compilation.

Synthesis applies each triggered consequence set transactionally. It repeats all relationships to
a bounded fixed point so that one consequence may trigger another implication. A cycle or conflict
that cannot settle is rejected by the complete validator; Simile never returns the intermediate
candidate as sound.

## Equality across fields and items

An equality constraint proves that every selected value is the same:

```yaml
- equal:
    paths:
      - /currency
      - /lines/data/*/currency
```

The compiler resolves the scalar schema at the end of every selector, requires compatible kinds,
and finds a value accepted by every participating schema. That common witness makes the relation
constructive during synthesis. An empty array contributes no selected values; ordinary OpenAPI
`required` and item-count rules continue to decide whether that collection may be empty.

## Numeric equations

An equation computes one numeric field from required numeric source fields:

```yaml
- equation:
    target: /amount_remaining
    value:
      subtract:
        - { path: /amount_due }
        - { path: /amount_paid }
```

The expression language is deliberately finite. A leaf is `{ path: /some/field }`; `add` accepts
two or more expressions and `subtract` accepts exactly two. Sources must be required, non-null numeric
properties and an equation may not read its own target. Integer-only arithmetic stays
integer-shaped when it fits the JSON integer range. Simile proves the computed result during
validation and writes that result into the target during synthesis.

Equations do not silently apply currency rounding, clamp negative values, or infer accounting
meaning. Those are separate declared relationships or ordinary schema bounds.

## Collection aggregates

An aggregate relates a numeric target to values selected from an array:

```yaml
- aggregate:
    target: /subtotal
    operation: sum
    source: /lines/data/*/amount
    when:
      path: /lines/has_more
      equals: false
```

`sum` is the currently supported operation. The source must contain an array wildcard and resolve
to non-null numeric items. An optional `when` uses the same field-predicate language as an implication. This
is important for paginated representations: a partial page must not claim to prove a whole-list
total. Without a guard, the selected collection is treated as complete; the sum of an empty
collection is zero.

## Lifecycle snapshots

A lifecycle constraint proves that a state and its transition timestamps describe one possible
path through a finite transition graph:

```yaml
- lifecycle:
    state: /status
    created-at: /created
    transitions:
      - { from: draft, to: open, at: /status_transitions/finalized_at }
      - { from: [open, uncollectible], to: paid, at: /status_transitions/paid_at }
      - { from: open, to: uncollectible, at: /status_transitions/marked_uncollectible_at }
      - { from: [open, uncollectible], to: void, at: /status_transitions/voided_at }
```

The state property must be required and use a finite scalar enum. The transition graph must be
acyclic, have at least one initial state, and make every enum state reachable. `created-at` is
required. Transition timestamps use the same numeric or `date-time` kind and must be nullable or
optional so timestamps outside the chosen path can be cleared.

Validation accepts a snapshot only when one declared path ends at the current state, every
timestamp on that path is present and non-null, timestamps outside it are null or absent, and the
path is temporally ordered from creation onward. Synthesis chooses a deterministic valid path,
establishes its timestamps, and clears incompatible branches. Cyclic workflows and event histories
that need repeated transitions require a different event-oriented relationship; they are not
misrepresented as this snapshot model.

## Derived string templates

A template ties a string field to required scalar fields:

```yaml
- template:
    path: /lines/url
    value: "/v1/invoices/{/id}/lines"
```

Every `{/...}` placeholder is a JSON Pointer. Its source must be a required, non-null string, number, or
boolean property. The target must be a string. Validation renders and compares the complete value;
synthesis writes the rendered string. Templates do not perform URL encoding or interpret prose.

## Extending relationships

The compiled constraint model is a typed enum, not a general expression interpreter. Each new
relationship must define source syntax, schema compilation, artifact validation, runtime proof,
and constructive synthesis. Unsupported operators, unresolvable selectors, incompatible value
kinds, dead witnesses, contradictory consequences, cyclic lifecycles, and other declarations that
cannot be executed safely fail compilation instead of surviving as inert metadata.

Future relationships such as products, minima and maxima, conditional equations, cardinality,
set membership across collections, and mutually exclusive branches should be added as explicit
typed variants. Simile will not infer them from property names or description prose.
