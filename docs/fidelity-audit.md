---
title: Fidelity audit
description: Find and explain where a service still relies on convention or unconstrained synthesis.
---

`simile audit` answers a narrower and more useful question than “Is the file valid?” It explains
where a valid artifact may still be an incomplete emulator.

```bash
simile audit service.similec
simile audit service.similec --strict
```

The report distinguishes errors, warnings, and information. Every finding has a stable code, a
specific operation or backend, a plain-language explanation, and an actionable remediation.
Current checks include:

- missing stable `operationId` values;
- operations whose outcomes remain schema-only;
- state-mutating operations with no backend effect;
- untyped or unsupported HTTP parameter styles;
- inferred effects, so reviewers can distinguish convention from authorship;
- backends that no action uses; and
- protected operations with no matching local profile;
- contract mechanisms, such as mutual TLS, that the listener cannot exercise;
- unsigned webhook delivery; and
- public protocol failures without provider error mappings.

The summary also counts secured operations, principals, webhook contracts, and signed webhook
policies. Operation details identify whether behavior came from the contract, provider-authored
HCL, or an inference. This provenance lets a reviewer focus on the places where Simile had to fill
in meaning.

`--strict` also treats warnings as failure, which is useful after a provider has established its
acceptable baseline. Without it, only audit errors fail the command. Machine-readable output is
available with `--output json`.

The audit is deliberately explainable rather than a proprietary score. Providers should be able
to improve a definition without learning an internal Simile decision tree, and reviewers should
be able to see exactly which fidelity claims remain inferred.
