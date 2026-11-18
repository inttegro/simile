---
title: Fault glossary
description: Catalog names, selectors, phases, actions, and execution terms used by Simile faults.
---

Use this page when authoring a service configuration or reading fault output. It names the faults
Simile can execute today and distinguishes them from domain errors, validation failures, and
asynchronous effects.

## Built-in catalog

Catalog entries are complete, provider-neutral fault definitions. Importing one copies its phase,
action, and default parameters into the compiled service artifact. Simile does not resolve the
catalog again at run time.

| Name | Phase | Action and default | Observable behavior | Allowed override |
| --- | --- | --- | --- | --- |
| `http.slow` | `before_response` | Delay for `1s` | The operation completes normally, then the HTTP response waits before delivery | `latency`, from `1ms` through `24h` |
| `http.response_lost` | `after_commit` | Disconnect | The operation commits, but the connection closes before the response reaches the caller | None |

Both entries are safe for deterministic scenario injection and bounded stochastic profiles. With
`http.response_lost`, an idempotent retry returns the committed outcome; Simile does not replay the
delivery failure.

<Note>
The current executable catalog contains the `http` namespace only. A selector such as `http2.*`
or `network.*` fails compilation until that namespace has real adapter behavior. Simile never
accepts a catalog name that would become a no-op.
</Note>

## Selector glossary

Selectors form a set. Positive selectors add catalog entries; selectors prefixed with `^` subtract
them from the complete configuration, including selections made by another behavior module.

| Form | Meaning | Example |
| --- | --- | --- |
| Exact name | Import one complete catalog fault | `use "http.slow" {}` |
| `namespace.*` | Import every fault currently in one namespace | `use "http.*" {}` |
| `*` | Import the complete executable catalog | `use "*" {}` |
| `^name` | Exclude one exact fault | `use "^http.response_lost" {}` |
| `^namespace.*` | Exclude an entire namespace | `use "^http.*" {}` |

For example, this imports the complete catalog except response loss:

```hcl
faults {
  use "*" {}
  use "^http.response_lost" {}
}
```

Exclusions always win, regardless of declaration or module order. Repeated unchanged imports are
idempotent. A selector that matches no catalog entry fails compilation, which catches misspelled
names and namespaces before the service binds a port.

Aliases and overrides apply only to exact positive imports:

```hcl
faults {
  use "http.slow" {
    as      = "very_slow"
    latency = "5s"
  }
}
```

Wildcards and exclusions cannot carry an alias or override because one declaration may refer to
several independently configured faults.

## Execution vocabulary

| Term | Meaning |
| --- | --- |
| Fault | Environmental behavior that changes how a completed service result reaches the caller, without selecting a different domain result |
| Catalog fault | A built-in fault with a stable name and sensible defaults |
| Custom fault | A provider-authored fault used when the built-in catalog cannot express the public testing contract |
| Phase | The boundary at which the transport action occurs |
| Action | The transport plan enacted by an adapter, currently `delay` or `disconnect` |
| Activation | One decision to apply one declared fault to one eligible operation attempt |
| Scenario injection | Deterministically applies a named fault to a named OpenAPI `operationId` |
| Fault profile | Applies declared environmental faults stochastically within explicit bounds |
| Seed | Reproduces a profile's pseudo-random activation sequence for the same compiled service and Simile release |
| Cooldown | Required eligible clean attempts after a profile reaches its consecutive-activation limit |
| Transport plan | Protocol-neutral runtime instruction that the HTTP adapter turns into waiting or disconnecting |

## Phases and actions

The supported combinations are deliberately narrow:

| Phase | Supported action | Meaning |
| --- | --- | --- |
| `before_response` | `delay` | Wait after operation execution and before writing the response |
| `after_commit` | `disconnect` | Preserve the committed outcome, then end delivery before the caller receives it |

Compilation rejects other combinations. This prevents a familiar label from hiding different
commit or retry semantics.

## Related concepts that are not faults

| Concept | Why it is separate |
| --- | --- |
| Outcome | Selects what the service returns, such as success, decline, or forbidden state |
| Validation failure | Rejects a request that does not satisfy the public contract |
| Protocol error | Maps malformed transport or runtime causes into a service-declared client response |
| Asynchronous effect | Declared work triggered by an operation, such as queueing mail or updating an abstract dependency |
| Generation heuristic | Improves synthesized data when no explicit relationship was published; it does not alter request validity |
| Replay | Returns a previously committed outcome for an idempotency key or stable resource identity |

Domain failures must be deterministic outcomes selected by public request predicates or opaque
scenarios. They must not appear in stochastic fault profiles: a valid payment method should never
become declined merely because random fault injection is active.

Read [Fault injection](/fault-injection) for configuration and execution guidance,
[Outcome fidelity](/outcome-fidelity) for domain-result selection, and
[State, identity, and replay](/state-and-replay) for retry semantics.
