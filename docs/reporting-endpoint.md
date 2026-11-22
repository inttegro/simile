---
title: Reporting endpoint
description: Implement the preflight and submission protocol used by Simile issue reports.
---

A reporting endpoint belongs to the service provider. It receives reproducible emulator defects
from people who explicitly chose to send them. The receiver should quarantine each submission,
verify its attached case in an isolated worker, and admit it to the provider's issue workflow only
after verification succeeds.

The repository includes a
[Cloudflare reference receiver](https://github.com/inttegro/simile/tree/master/receiver/cloudflare)
that uses D1 for report state, R2 for quarantined evidence, a Queue for verification work, and a
private verifier service binding. Reporter contact is encrypted before it enters D1.

The `reporting.endpoint` value names the submission route:

```hcl
reporting {
  endpoint = "https://reports.example.com/v1/reports"
}
```

Simile derives the preflight route by appending `/preflight`.

## Preflight

Simile sends `POST /v1/reports/preflight` with JSON:

```json
{
  "simileVersion": "0.3.0",
  "service": "inttegro",
  "serviceVersion": "2026-11-22",
  "artifactDigest": "1fba24f7b8a06d5d…"
}
```

The response lists available upgrades:

```json
{
  "recommendations": [
    {
      "component": "service",
      "current": "2026-11-22",
      "available": "2026-11-24",
      "command": "simile update inttegro"
    }
  ]
}
```

Return an empty array when the captured versions are current. Simile saves any recommendation and
pauses submission unless the user passes `--send-current`.

## Submission

Simile sends `POST /v1/reports` as `multipart/form-data`. The complete request is limited to 8 MiB.

| Part | Media type | Purpose |
| --- | --- | --- |
| `report` | `application/json` | Technical report with no contact address |
| `reporter` | `application/json` | Contact address and clarification/update consent |
| `bundle.json` | `application/json` | Reproduction manifest |
| `request.json` | `application/json` | Request media type and parameters |
| `request.body` | `application/octet-stream` | Sanitized request body, when present |
| `service.similec` | `application/octet-stream` | Exact compiled artifact used by the run |

The reporter part has this form:

```json
{
  "contact": {
    "channel": "email",
    "address": "developer@example.com"
  },
  "consent": {
    "clarification": true,
    "resolutionUpdates": true
  }
}
```

Keep this document outside the technical report store. It can be deleted independently when the
case is resolved or the retention period expires.

## Receipt and verification

After validating the envelope and placing attachments in quarantine, return:

```json
{
  "id": "rpt_8eaadf11a66373a4",
  "status": "pending_verification"
}
```

The receiver then runs the case with a compatible Simile verifier in an isolated environment. It
must confirm all of the following before accepting the report:

- the `.similec` envelope and digest are valid;
- the artifact digest agrees across the report and bundle;
- the request can be decoded and validated by the bundled operation;
- execution produces the recorded diagnostic with the recorded seed and controls; and
- the verification fingerprint agrees with the report.

Verification failure should mark the submission `rejected` and keep it out of the provider's issue
tracker. Successful verification may mark it `accepted`, notify maintainers, and retain the contact
only for clarification and resolution updates.

## Receiver safeguards

A public receiver should apply request-size limits, rate limits, strict multipart names, private
object storage, short quarantine retention, and per-report access logs. Treat `.similec` and request
attachments as untrusted bytes. Verification belongs in a process with no production credentials,
no internal network access, bounded CPU and memory, and a disposable filesystem.

Report IDs are supplied by Simile and use the `rpt_` prefix. A duplicate identifier should be
idempotent only when every stored digest agrees; otherwise return a conflict.

Read [Report emulator issues](/issue-reporting) for the sender workflow.
