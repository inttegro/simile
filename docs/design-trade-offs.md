---
title: Design trade-offs
description: Decide which integration tests belong in Simile and understand the engineering costs behind that decision.
---

Simile moves routine integration testing from provider-hosted infrastructure into a versioned
service model that runs on a developer machine or CI worker. This gives teams faster, isolated,
and reproducible tests, while leaving a focused set of checks on provider-owned infrastructure.
This guide explains the boundary, the guarantees Simile can make, and the costs required to keep
those guarantees credible. {{ className: 'lead' }}

## Prerequisites

- Understand the service's published HTTP contract and the workflows your integration depends on.
- Read [Capabilities and limits](/current-boundaries) for Simile's current product boundary.
- Identify which existing tests depend on provider credentials, network policy, regional
  configuration, or undocumented behavior.

## Choose the evidence each test needs

The central design decision is where a test should obtain its evidence. An emulator can give a
strong answer when expected behavior follows from a public contract, a provider-authored outcome,
or a reviewed example. The provider remains the authority for properties created by its live
infrastructure.

That distinction produces three useful testing layers:

| Layer | Evidence it can supply | Examples |
| --- | --- | --- |
| Local Simile tests | Behavior compiled into the service definition | Request validation, documented errors, request-response relationships, state transitions, idempotency, retries, and recovery |
| Provider-hosted tests | Behavior controlled by the provider environment | Real credentials, IAM or equivalent permissions, account configuration, regional differences, network policy, and provider-managed integrations |
| Production-safe smoke tests | Behavior of the deployed application in its real path | Endpoint reachability, deployment configuration, and a small number of reversible end-to-end checks |

The first layer should carry most of the test volume because it is cheap to repeat and easy to
isolate. The second and third layers should remain small enough that their external access,
credentials, latency, and cleanup requirements are deliberate.

[AWS Emulator](https://docs.aws.amazon.com/emulator/latest/userguide/what-is-service.html) provides
useful provider-side evidence for this model. AWS describes a local API endpoint that runs on a
developer machine or in CI, requires no AWS account, and creates no AWS resources. AWS also directs
users to run security and final application checks on real AWS. A major provider making both
statements explicit supports local emulation as a credible testing category while preserving the
authority of provider-hosted validation.

This boundary should be decided per behavior, rather than per service. A payment integration can
exercise request validation, a documented decline, idempotency, and webhook retry locally, while a
small hosted suite verifies the real account's permissions and webhook endpoint configuration.
Moving the broad suite local does not require pretending that every provider property is
emulatable.

## Understand what Simile guarantees

Simile's implementation is designed to make the local evidence inspectable. Each decision that
improves repeatability also creates an obligation for the provider or the emulator maintainers.

### Compilation creates a stable execution boundary

Simile compiles OpenAPI, Smithy, or semantic TypeScript, plus optional behavior HCL, into an
immutable `.similec` artifact. The runtime executes the artifact without reopening or reinterpreting
the source files. Invalid operation references, unsupported declarations, contradictory rules, and
unconstructible authored outcomes can therefore fail before a test starts.

Compilation makes CI runs easier to reproduce and lets reviewers inspect the exact model under
test. The cost is an explicit build step and an artifact compatibility policy. A changed contract
must be recompiled, distributed, and tested. A valid artifact can still be incomplete when its
source contract omits behavior that matters to an integration. Use [Fidelity audit](/fidelity-audit)
to find schema-only outcomes, inferred effects, and other places where review is still required.

### Synthesis proves a result against published rules

For generated responses, Simile performs bounded construction and validates the complete candidate
before delivery. Required fields, request bindings, explicit relationships, selected outcomes, and
state rules participate in that final check. When the compiled rules conflict or finite search
cannot construct a valid candidate, the safe result is an `emulation_failed` diagnostic. Returning
a plausible object would let an impossible state pass unnoticed.

This approach gives stronger evidence than static fixtures for values that must vary or reconcile.
It also means complex definitions sometimes need a provider-authored example or named outcome.
Minimal generation fills the fields required by the selected contract and its dependencies;
optional product detail appears only when the definition supplies evidence for it. See
[Payload soundness](/payload-soundness) and
[Request-response relations](/request-response-relations) for the construction model.

### State and consequences stay local

Simile can persist resources, cache entries, queued work, mail, objects, scheduled work, logical
time, and causal history in an embedded store. Typed effects connect a successful operation to its
synchronous writes and durable follow-up work. Local persistence makes restart and recovery tests
possible without provisioning databases, queues, or worker services for every test run.

One local service process cannot reproduce a provider's distributed topology, contention profile,
capacity controls, or regional failure modes. Providers must also author consequences that cannot
be inferred from an HTTP schema. This is an implementation trade-off: a smaller operational
surface in exchange for a model whose distributed limits remain visible.

### Provenance keeps fidelity claims reviewable

Simile distinguishes contract-derived, provider-authored, convention-inferred, and observed
behavior. A provider example carries different evidence from a schema-generated response, and a
proxy observation remains a draft until someone reviews it. `simile describe service --verbose`
and `simile audit` expose those origins.

Provenance adds metadata, review work, and authoring discipline. It prevents a more expensive
failure: presenting every schema-valid response as an exact copy of production. AWS takes a
comparable evidence-oriented approach by publishing
[fidelity definitions and per-service scores](https://docs.aws.amazon.com/emulator/latest/userguide/fidelity.html).
AWS describes each score as the number of published fidelity tests passed for a service and warns
that fidelity varies by service and feature. Simile uses findings and provenance instead of a
single score, but the underlying principle is the same: compatibility needs visible evidence.

## Account for the implementation burden

Moving tests local transfers work from hosted environment operation to definition and emulator
maintenance. Plan for all of the following:

- **Provider authorship.** Public lifecycle rules, documented failures, state effects, and
  important examples need a reviewed source.
- **Conformance maintenance.** Provider examples and selected live checks must detect drift as the
  contract and observable behavior evolve.
- **Runtime resources.** Developer machines and CI workers provide CPU, memory, storage, and cache
  capacity for the emulator.
- **Tool distribution.** Teams need pinned Simile releases, immutable service definitions, and a
  process for adopting compatible revisions.
- **Fidelity investigation.** A difference between local and provider behavior needs triage,
  evidence, and sometimes a definition or runtime change.
- **Security discipline.** The emulator must remain private, use synthetic credentials, and avoid
  production data and real provider secrets.

Local resource cost can be material. The AWS Emulator repository asks source builders for
[at least 100 GB of free disk for its Rust build output](https://github.com/aws/aws-emulator#prerequisites)
and recommends prebuilt packages for most users. That figure describes the scope of AWS's emulator,
not Simile's resource requirement. It is still a useful reminder that local infrastructure moves
compute and storage onto developer and CI systems.

Proxy learning creates another deliberate cost. Captured traffic can accelerate a draft contract,
but observations are sample-biased and may contain sensitive business values. Simile redacts
recognized credentials, omits free-form and binary bodies, and still requires a human review before
the learned definition becomes authoritative. Read
[Evidence and limits](/proxy-evidence-and-limits) before collecting or sharing a learning workspace.

## Keep the validation portfolio explicit

A trustworthy adoption plan names what moved, what stayed, and why. Start with workflows whose
expected outcomes are public and stable. Record the provider checks retained for identity,
permissions, account configuration, networking, regional behavior, and any feature with an
unresolved fidelity finding. Keep production smoke tests reversible and small.

AWS's security guidance provides a clear outer boundary: passing emulator tests does not establish
that IAM, resource policies, KMS policies, cross-account access, or other security controls are
correct. The guide instructs teams to
[test security settings and the complete application on real AWS](https://docs.aws.amazon.com/emulator/latest/userguide/security.html).
The same rule applies to Simile. Local tests establish behavior within the compiled service model;
they do not certify controls owned by a production provider.

This portfolio keeps each layer accountable for evidence it can actually produce. It also gives
teams a practical migration path: expand local coverage as the definition gains reviewed behavior,
and retain external checks wherever the evidence still belongs to the provider.

## Related resources

- [Why Simile exists](/why-simile) - The product boundary and the case for provider-published local behavior
- [Risks of hosted test environments](/hosted-test-environment-risks) - How non-production trust can reach production systems
- [Test infrastructure costs](/test-infrastructure-costs) - A complete model for money, time, security, and fidelity work
- [Conformance suites](/conformance-suites) - Turn provider examples into deterministic compatibility checks
- [CI testing](/ci-testing) - Run one isolated Simile service for each CI job
