---
title: Lifecycles and templates
description: Finite state transitions and strings rendered from required scalar fields.
---

`lifecycle` and `template` cover structured relationships that are more specific than comparison or
arithmetic.

## `lifecycle`

Use `lifecycle` when a finite state and nullable or optional transition timestamps describe one
acyclic path through a resource's history.

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

The state must be a required finite scalar enum. The graph must be acyclic, every state must be
reachable, and transition timestamps must use the same numeric or date-time kind as `created-at`.

During proof, Simile checks that the visible timestamps describe a valid path to the declared
state. During construction, it chooses a valid path and establishes the corresponding timestamp
presence and ordering.

## `template`

Use `template` when a string is derived from required, non-null scalar fields.

```yaml
- template:
    path: /lines/url
    value: "/v1/invoices/{/id}/lines"
```

Each `{/...}` placeholder is a JSON Pointer relative to the object containing the extension. Simile
renders the complete string; it does not URL-encode values or interpret description prose.

Return to [OpenAPI extensions](/openapi-extensions) for the complete clause map, or read
[Payload soundness](/payload-soundness) for the architecture behind proof and construction.
