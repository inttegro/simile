---
title: Conformance suites
description: Turn executable provider examples into a deterministic compatibility gate.
---

A conformance suite is a directory of [reproduction bundles](/reproduction-bundles). Each bundle
is one executable claim: with this compiled service, normalized request, seed, and public control,
the operation produces this response.

```text
provider-examples/
├── create-order/
│   ├── bundle.json
│   ├── request.json
│   └── service.similec
└── decline-payment/
    ├── bundle.json
    ├── request.json
    └── service.similec
```

Run a directory or repeat bundle arguments:

```bash
simile conformance ./provider-examples
simile conformance ./create-order ./decline-payment
```

The command discovers direct child bundles, verifies every one independently, prints the operation
and bundle responsible for a failure, and exits unsuccessfully if any case diverges. JSON output is
available for CI:

```bash
simile --output json conformance ./provider-examples
```

Conformance proves that the published Simile artifact still satisfies those executable examples.
It does not compare against a live production system or reveal the provider's private decision
tree. Providers should select cases that represent stable public promises: successful creation,
documented errors, lifecycle transitions, idempotency, and meaningful scenarios.
