---
title: Fault injection
description: Test transport failures without turning valid requests into random domain errors.
---

Faults change **how** a completed service result reaches the caller. They do not change **which**
result the service produces. That distinction keeps Simile useful for integration tests:

- a declined payment, invalid parameter, or forbidden state is a deterministic outcome selected by
  a public request condition or an opaque scenario;
- high latency and a lost connection are environmental faults applied to an operation attempt; and
- asynchronous work is declared separately and is not a fault.

Simile models those concepts separately all the way from HCL to the HTTP adapter. A stochastic
profile can therefore disrupt delivery, but it cannot invent a bad payment method when the caller
sent a valid one.

<Note>
Need an exact catalog name or execution term? Use the [Fault glossary](/fault-glossary).
</Note>

```mermaid
flowchart LR
  Request[Validated request] --> Select[Select service outcome]
  Select --> Commit[Complete and record outcome]
  Commit --> Fault{Fault for this attempt?}
  Fault -->|none| Reply[Send response]
  Fault -->|delay| Wait[Wait, then send response]
  Fault -->|disconnect after commit| Drop[Close delivery stream]
  Commit --> Replay[Replay index]
  Replay --> Retry[Idempotent retry returns committed outcome]
```

## Use the built-in HTTP catalog

Simile includes complete, provider-neutral HTTP faults with sensible defaults. Select only the
faults the service needs:

```hcl
faults {
  use "http.slow" {}
  use "http.response_lost" {}
}
```

Catalog selection is set-based. Import the complete catalog in one line:

```hcl
faults {
  use "*" {}
}
```

Import one complete namespace with a suffix wildcard:

```hcl
faults {
  use "http.*" {}
}
```

Prefix a selector with `^` to subtract it. Exclusions apply to the complete configuration, so a
later positive selector cannot accidentally add the fault again:

```hcl
faults {
  use "*" {}
  use "^http.response_lost" {}
}
```

Exact selectors, namespace selectors, and exclusions may be composed across inline configuration
and imported behavior modules. Repeated selection of the same unchanged fault is idempotent.
Selectors that match nothing fail compilation, catching misspelled names and namespaces.

`http.slow` delays a response by one second. `http.response_lost` completes and commits the
operation, then closes delivery before the response reaches the caller. Catalog entries compile
into the same self-contained artifact records as provider-authored faults; starting the service
does not fetch or resolve a remote catalog.

Defaults are optional, not restrictive. Override only the property that matters to the service:

```hcl
faults {
  use "http.slow" {
    latency = "2s"
  }
}
```

Latency accepts whole-number durations from `1ms` through `24h`, using `ms`, `s`, `m`, or `h`.
An exact import keeps its catalog name, so scenarios and profiles refer directly to `http.slow`.
Use an alias when the same preset needs more than one configuration:

```hcl
faults {
  use "http.slow" {}
  use "http.slow" {
    as      = "very_slow"
    latency = "5s"
  }
}
```

Unknown catalog names, unsupported overrides, aliases that collide with another fault, and
invalid durations fail compilation. Wildcards and exclusions cannot contain aliases or overrides;
configure an exact import instead. Simile never silently substitutes a different fault.

## Define a custom fault

The initial action vocabulary is intentionally small. Define a custom fault when a catalog preset
does not express the service's public testing contract:

```hcl
fault "slow_response" {
  phase = "before_response"

  delay {
    milliseconds = 250
  }
}

fault "response_lost" {
  phase = "after_commit"
  disconnect {}
}
```

`before_response` currently accepts `delay`; `after_commit` currently accepts `disconnect`.
Compilation rejects mismatched phase/action pairs. A delay must be from 1 millisecond through 24
hours. These narrow combinations make ordering explicit and prevent a transport label from quietly
changing service semantics.

## Deterministic scenarios

Use a scenario when a test must reproduce the same disruption every time:

```hcl
scenario "lost_after_commit" {
  inject "createOrder" {
    fault = "http.response_lost"
  }
}
```

The label on `inject` is an exact OpenAPI `operationId`. The referenced fault must exist. A scenario
may also select named outcomes, so one opaque test setup can choose a public service result and a
delivery failure without exposing a provider's private decision tree.

```console
$ simile start inttegro --scenario lost_after_commit
```

Scenario injection is attempt-scoped. If `createOrder` commits and the connection is lost, a retry
with the same idempotency key returns the stored result normally. Simile stores the operation
outcome in the replay index, not the transport failure that affected its first delivery.

## Bounded stochastic profiles

Use a profile to exercise resilience over a longer local or CI session:

```hcl
fault_profile "rough_network" {
  probability       = 0.02
  faults            = ["http.slow", "http.response_lost"]
  operations        = ["createOrder"]
  max_consecutive   = 1
  cooldown_requests = 20
  seed              = 42
}
```

- `probability` is per eligible attempt, from `0.000001` through `1`.
- `faults` is a non-empty set of declared environmental faults.
- `operations` is an optional list of OpenAPI `operationId` values; omit it to cover all operations.
- `max_consecutive` defaults to `1` and limits uninterrupted activations.
- `cooldown_requests` defaults to `20` and counts eligible clean attempts after that limit.
- `seed` is optional. When omitted, Simile creates a session seed and reports it at startup.

```console
$ simile start inttegro --fault-profile rough_network
Started inttegro 1.0.0
listen http://127.0.0.1:4242
faults rough_network · seed 17342879984032484678
```

Record the reported seed with a failing test. Adding it to the same profile reproduces the random
sequence for the same compiled service and Simile release. `--output json` includes
`activeFaultProfile` and `faultSeed` so CI can retain the same evidence without parsing human
output.

Profiles cannot name domain outcomes and currently support only transport-safe fault actions. This
is deliberate: random network adversity is useful; random business rejection makes an integration
test untrustworthy.

## Runtime and adapter boundary

The runtime completes an `OperationOutcome` first, then returns an `OperationExecution` containing
an optional fault activation and a protocol-neutral transport plan. The replay store contains only
the outcome. `simile-server` owns the concrete HTTP behavior: sleeping before a response or ending
the response stream after commit.

That split lets future adapters implement the same plan without importing HTTP into the runtime,
and lets future fault actions grow without becoming response-selection rules.

Read [Outcome fidelity](/outcome-fidelity) for deterministic error selection,
[Fault glossary](/fault-glossary) for catalog names and terminology,
[State, identity, and replay](/state-and-replay) for retry semantics, and
[Operation execution](/operation-execution) for the runtime pipeline.
