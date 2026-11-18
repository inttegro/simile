---
title: Fault glossary
description: Complete fault names, availability, scopes, selectors, phases, and execution terms.
---

Use this page when authoring a service configuration, planning a resilience test, or reading fault
output. It records the complete designed catalog, identifies which entries Simile can execute
today, and distinguishes faults from domain errors, validation failures, and asynchronous effects.

## Catalog status

Catalog entries are complete, provider-neutral fault definitions. Importing one copies its phase,
action, and default parameters into the compiled service artifact. Simile does not resolve the
catalog again at run time.

The HTTP and TLS catalog currently has **25 defined entries**: **2 available** and **23 planned**.
Only entries marked **Available** are legal in `faults { use ... }`. Planned names document the
stable direction and intended semantics; the compiler rejects them until the relevant adapter can
perform the fault truthfully.

<Warning>
Do not use a planned entry as if it were executable. Simile deliberately fails an unmatched exact
name or namespace instead of compiling a fault that does nothing.
</Warning>

## Available now

| Name | Phase | Action and default | Observable behavior | Allowed override |
| --- | --- | --- | --- | --- |
| `http.slow` | `before_response` | Delay for `1s` | The operation completes normally, then the HTTP response waits before delivery | `latency`, from `1ms` through `24h` |
| `http.response_lost` | `after_commit` | Disconnect | The operation commits, but the connection closes before the response reaches the caller | None |

Both entries are safe for deterministic scenario injection and bounded stochastic profiles. With
`http.response_lost`, an idempotent retry returns the committed outcome; Simile does not replay the
delivery failure.

## Complete HTTP and TLS catalog

### Timing and flow

| # | Name | Availability | Intended scope | Observable behavior |
| --- | --- | --- | --- | --- |
| 1 | `http.slow` | **Available** | operation | Delay the completed response before delivery. |
| 2 | `http.slow_request_read` | Planned | request stream | Consume the request body slowly enough to exercise caller and server time budgets. |
| 3 | `http.slow_response_headers` | Planned | response headers | Delay only the response headers. |
| 4 | `http.slow_response_body` | Planned | response stream | Send the response body gradually after headers arrive. |
| 5 | `http.request_read_timeout` | Planned | request stream | Time out while consuming the request body. |
| 6 | `http.response_write_timeout` | Planned | response stream | Time out while writing the response. |
| 7 | `http.stalled_response` | Planned | response stream | Begin processing or delivery but never complete the response. |

### Connection lifecycle

| # | Name | Availability | Intended scope | Observable behavior |
| --- | --- | --- | --- | --- |
| 8 | `http.connection_closed_before_response` | Planned | connection | Close the connection cleanly without returning a response. |
| 9 | `http.connection_reset_before_response` | Planned | connection | Reset the connection before response delivery begins. |
| 10 | `http.connection_reset_during_response` | Planned | response stream | Reset the connection after partial response delivery. |
| 11 | `http.response_lost` | **Available** | after commit | Commit successfully, then close before the caller receives the response. |
| 12 | `http.keep_alive_closed` | Planned | connection | Unexpectedly close a connection the caller expected to reuse. |
| 13 | `http.connection_refused` | Planned | listener | Refuse the connection before an HTTP operation exists. |

### Response integrity

| # | Name | Availability | Intended scope | Observable behavior |
| --- | --- | --- | --- | --- |
| 14 | `http.truncated_response` | Planned | response stream | Send only part of the declared response body. |
| 15 | `http.malformed_body` | Planned | response body | Return bytes invalid for the declared representation. |
| 16 | `http.malformed_headers` | Planned | response headers | Emit invalid response headers. |
| 17 | `http.incorrect_content_length` | Planned | response headers | Declare a length different from the transmitted body. |
| 18 | `http.corrupt_compression` | Planned | response stream | Return invalid compressed content. |
| 19 | `http.wrong_content_type` | Planned | response headers | Return bytes under an incompatible media type. |
| 20 | `http.invalid_chunk_encoding` | Planned | HTTP/1.1 stream | Emit invalid chunk framing. |

### HTTP/2

| # | Name | Availability | Intended scope | Observable behavior |
| --- | --- | --- | --- | --- |
| 21 | `http2.rst_stream` | Planned | HTTP/2 stream | Reset the active stream. |
| 22 | `http2.goaway` | Planned | HTTP/2 session | Terminate the connection with `GOAWAY`. |
| 23 | `http2.refused_stream` | Planned | HTTP/2 stream | Refuse a stream in a way that permits a caller retry. |

### TLS

| # | Name | Availability | Intended scope | Observable behavior |
| --- | --- | --- | --- | --- |
| 24 | `tls.handshake_timeout` | Planned | TLS handshake | Stall until the caller's handshake deadline expires. |
| 25 | `tls.handshake_failure` | Planned | TLS handshake | Terminate the handshake unsuccessfully. |

The scope column matters. Operation-scoped faults can be attached after route selection.
Connection-, listener-, HTTP/2-, and TLS-scoped faults require controls outside the current
operation runner. They will become importable only with a scope model and adapter implementation
that preserve their actual retry and commit semantics.

## Planned subsystem namespaces

Wire faults are only one half of the catalog. Simile's resource abstractions also need faults for
observable dependency and async-work behavior. These namespaces remain design vocabulary rather
than executable selectors until deterministic resource adapters exist.

| Namespace | Planned behavior families |
| --- | --- |
| `network.*` | latency, partition, packet loss, bandwidth limits, and DNS failure |
| `db.*` | unavailability, timeout, deadlock, write conflict, stale read, and partial commit |
| `cache.*` | miss, unavailability, timeout, stale value, and eviction |
| `queue.*` | publish failure, delayed delivery, duplicate delivery, redelivery, dropped messages, and reordering |
| `mail.*` | rejection, bounce, delayed delivery, rate limiting, and provider unavailability |
| `object_store.*` | unavailability, timeout, missing object, corrupt read, and interrupted write |
| `event_bus.*` | failed publication, delayed event, duplicate event, and reordering |
| `scheduler.*` | delayed, missed, and duplicate execution |
| `process.*` | crash, restart, overload, and graceful shutdown during work |
| `clock.*` | skew, freeze, forward jump, and backward jump |

These remain environmental or subsystem faults. Service-specific failures such as a declined
payment, depleted balance, or forbidden state remain deterministic outcomes.

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
