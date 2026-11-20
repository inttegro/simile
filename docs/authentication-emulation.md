---
title: Authentication emulation
description: Exercise published security requirements with local identities, credentials, grants, and lifecycle states.
---

Authentication begins in the API contract. OpenAPI names each mechanism, tells clients where to
present credentials, and attaches security requirements to operations. Simile preserves that
structure and lets the service author provide local test identities in HCL.

```mermaid
flowchart LR
  Contract[OpenAPI security schemes] --> Requirement[Operation requirement]
  Principal[HCL test principal] --> Match[Credential and grant match]
  Request[Incoming headers, query, or cookies] --> Match
  Requirement --> Match
  Match -->|accepted| Operation[Operation execution]
  Match -->|missing or unknown| Unauthorized[401]
  Match -->|known identity lacks grants| Forbidden[403]
```

## Publish the security contract

```yaml
components:
  securitySchemes:
    merchantKey:
      type: apiKey
      in: header
      name: X-Merchant-Key
    merchantOAuth:
      type: oauth2
      flows: {}

paths:
  /orders:
    post:
      operationId: createOrder
      security:
        - merchantOAuth: [orders:write]
        - merchantKey: []
```

The two array entries are alternatives. A bearer credential with `orders:write` can satisfy the
first entry, while the API key can satisfy the second. Schemes inside one entry compose with
logical AND. This follows OpenAPI's native OR-of-AND model and avoids a second security language.

An operation-level `security: []` makes that operation public even when the document declares
global security.

## Declare local principals

```hcl
principal "merchant_app" {
  description = "A local merchant integration with order write access."
  grants      = ["orders:write"]

  credential "merchantOAuth" {
    value = "sim_test_merchant"
  }
}

principal "revoked_key" {
  description = "A key retained for revocation handling tests."
  status      = "revoked"

  credential "merchantKey" {
    value = "sim_test_revoked"
  }
}
```

The credential label refers to an OpenAPI security-scheme name. The compiler checks that the
credential form fits the scheme:

- API keys, bearer tokens, OAuth 2.0, and OpenID Connect use `value`.
- HTTP Basic uses `username` and `password`.
- `status` accepts `active`, `revoked`, or `expired` and defaults to `active`.
- `grants` contains the scopes that this local identity may exercise.

Use recognizable local fixtures such as `sim_test_*`. The compiled artifact contains those fixture
values so the emulator can compare requests. Keep production credentials out of configuration,
artifacts, examples, and source control. `simile describe service` lists principal names, statuses,
schemes, and grants while withholding credential values.

## Send authenticated requests

```console
$ curl http://127.0.0.1:4242/orders \
    -H 'Authorization: Bearer sim_test_merchant' \
    -H 'Content-Type: application/json' \
    --data '{"customer_id":"cu_test"}'
```

Authentication runs after route matching and before parameter or body validation. This ordering
lets a protected endpoint reject an unknown caller without revealing contract details from deeper
validation.

| Condition | Result |
| --- | --- |
| no declared security requirement | operation remains public |
| a complete alternative matches one active principal | operation executes |
| credential is missing, unknown, revoked, or expired | `401 authentication_required` |
| credential identifies an active principal without the required grants | `403 permission_denied` |

Provider-authored `protocol_error` mappings can replace Simile's fallback envelopes for
`authentication_required` and `permission_denied`.

## Current boundary

API keys in headers, query strings, and cookies are supported, together with HTTP Basic, HTTP
Bearer, OAuth 2.0 bearer tokens, and OpenID Connect bearer tokens. Mutual TLS appears in compiled
contracts. The fidelity audit marks each affected operation because the listener currently has no
verified client-certificate input.

Continue with [Configuration](/service-configuration) for composition, [Fidelity audit](/fidelity-audit)
for coverage checks, or [Current boundaries](/current-boundaries) for the full support map.
