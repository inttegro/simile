---
title: Reproduction bundles
description: Capture one execution as a portable, deterministic debugging artifact.
---

A seed alone is not a reproduction. The contract may change, an example may move, or a generated
request may select a different operation. A Simile reproduction bundle freezes the inputs required
to explain and repeat one execution.

## Capture a run

```bash
simile run ./openapi.yaml \
  --operation-id createOrder \
  --seed 42 \
  --bundle ./create-order.simile-repro
```

The directory is installed atomically and contains:

| File | Purpose |
| --- | --- |
| `bundle.json` | bundle version, Simile version, artifact format, artifact digest, operation, seed, controls, and expected response |
| `service.similec` | the exact compiled service used by the run |
| `request.json` | the normalized provided or synthesized request |

Simile refuses to overwrite an existing bundle. That makes a bundle suitable for an issue,
regression fixture, or CI artifact without silently replacing the evidence under review.

Simile also creates incident cases for verified issue reports. An incident case uses bundle format
2 and records the expected diagnostic, request parameters, sanitized request bytes, scenario, and
fault profile. It carries the same exact `.similec` artifact and digest as a response reproduction.
`simile replay` handles response bundles; the reporting pipeline verifies incident cases before
submission and the receiver verifies them again before acceptance.

## Replay it

```bash
simile replay ./create-order.simile-repro
```

Replay verifies the artifact envelope and digest, checks bundle and artifact compatibility,
re-executes the operation with the recorded seed and controls, and compares the complete response.
It exits unsuccessfully when the response diverges.

The bundle contains request and response values. Review a manually captured bundle before sharing
when those values came from an application. Automatic incident capture removes recognized secret
fields and skips opaque request bodies that it cannot sanitize safely.

## Why the artifact travels with the request

Recompiling the newest provider definition would answer a different question: “What happens now?”
Replaying the bundled artifact answers “Why did this exact test produce this result?” Both are
useful, but they are not interchangeable.

Use [Conformance suites](/conformance-suites) to run many bundles as one provider or integration
test gate. Use [Report emulator issues](/issue-reporting) to turn a verified bundle into a report.
