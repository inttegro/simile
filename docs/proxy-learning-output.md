---
title: Review learned output
description: Review the files created by a proxy learning session.
---

Press Ctrl-C to end a learning session. Simile writes the session under:

```text
~/.simile/learning/<service>/<session>/
```

The directory contains:

```text
evidence/
└── observations.jsonl
learning-report.json
service/
├── Simfile
├── behavior.hcl
└── openapi.yaml
```

## What each file contains

| File | Contents |
| --- | --- |
| `observations.jsonl` | One sanitized record for each API call. |
| `learning-report.json` | Counts of operations, statuses, bindings, and remaining gaps. |
| `openapi.yaml` | The operations, request shapes, and response shapes Simile observed. |
| `behavior.hcl` | A place to describe behavior that HTTP traffic cannot reveal. |
| `Simfile` | The service configuration that points to the generated contract and behavior. |

Simile compiles the generated service before it reports success.

The evidence stays beside the generated files. This makes each inferred operation, schema, and
binding traceable to the exchanges that produced it.

## Review the draft

Start with `learning-report.json`. Then inspect `openapi.yaml`.

The report summarizes what Simile saw and what it could infer. A small gap count does not prove that
the service is complete. It only describes the recorded session.

Check these details:

- Did Simile observe every important success and error response?
- Do fields have the right types? Are the correct fields required?
- Should any exact path segment become a path parameter?
- Does the service need lifecycle rules, asynchronous work, or faults in `behavior.hcl`?

Exact paths deserve special attention. If Simile observed `/customers/cu_123`, the draft may contain
that exact path. Change it to `/customers/{customer_id}` when the segment identifies a resource.

Required fields also need review. A field that appeared in every recorded response may still be
optional in the provider's full contract.

Simile adds a request-to-response binding after at least two observations show the same
relationship. Repeat an operation when its response should preserve a request ID, reference,
amount, or similar value.

Use `behavior.hcl` for facts that isolated HTTP exchanges cannot prove. These include lifecycle
transitions, stored state, delayed work, retries, and faults.

## Run the draft

Run the generated service directly:

```bash
simile start acme \
  --config ~/.simile/learning/acme/SESSION/service/Simfile
```

Register it when you are ready to reuse it:

```bash
simile register acme \
  --config ~/.simile/learning/acme/SESSION/service/Simfile
```

Registration installs the compiled service under the name `acme`. You can then run it with:

```bash
simile start acme
```

Treat refinement as an iterative cycle. Run the draft, compare its responses with the recorded
provider evidence, correct the contract and behavior, then repeat the important application flows.

Continue with [Define a service](/define-a-service) to refine the draft. Use
[Fidelity audit](/fidelity-audit) to find contract gaps.
