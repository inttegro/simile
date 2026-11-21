---
title: Evidence and limits
description: Learn what Simile saves and which proxy features are available today.
---

Simile saves a sanitized record of each observed API call. It uses these records to build the draft
OpenAPI document.

Each learning session has its own directory under `~/.simile/learning`. Starting a new session does
not overwrite earlier evidence.

## What Simile saves

| Data | Saved form |
| --- | --- |
| Request and response metadata | Method, path, status, media type, size, and elapsed time. |
| JSON bodies | Values used for schema and relationship inference. Recognized secret fields are redacted. |
| Credential headers and query values | Redacted. |
| Free-form text bodies | Content omitted. Shape and size retained. |
| Binary bodies | Content omitted. Shape and size retained. |

Review `evidence/observations.jsonl` before you share a learning workspace. JSON can contain private
business data even after Simile removes recognized credentials.

Simile recognizes common credential names such as `authorization`, `api_key`, `token`, and
`password`. A provider can use a different name. Unknown secret fields may remain in JSON, headers,
or query parameters, so treat the learning directory as private data.

For free-form text and binary bodies, Simile keeps metadata such as media type and size. It omits
the content. This protects the raw body, but it also limits what Simile can infer about its schema.

## Current limits

The proxy currently handles buffered HTTP traffic for local development.

- A request or response body can be up to 16 MiB.
- Paths remain exactly as Simile observed them.
- Request-to-response bindings require repeated observations.
- Simile cannot infer private state changes or asynchronous work from HTTP traffic alone.
- Streaming bodies, WebSockets, and `CONNECT` tunnels are not supported yet.

The 16 MiB limit applies separately to request and response bodies. Simile rejects an oversized
request before forwarding it. An oversized provider response ends as an upstream failure. Simile
does not learn from a partial body.

The proxy buffers bodies before it records or forwards them. This keeps learning deterministic and
makes size limits predictable. It also means the current proxy is unsuitable for large downloads,
server-sent events, and other long-lived streams.

## What the draft proves

The generated draft describes the traffic Simile observed. It does not prove that every provider
operation, status, field, or lifecycle transition has been covered. Provider documentation and
domain knowledge are still needed to turn the draft into a complete emulator.

Production capture will be designed separately. Use the current proxy for local development and
service discovery.

See [Review learned output](/proxy-learning-output) for the next step. See
[Current boundaries](/current-boundaries) for limits that apply to the rest of Simile.
