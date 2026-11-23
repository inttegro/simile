---
title: State inspection
description: Follow backend causality while keeping stored payloads private by default.
---

Registered services persist Stack state under `SIMILE_HOME`. `simile state` gives an operator a
single read-only view of the declared backends and their causal journal:

```bash
simile state inttegro
```

Journal entries contain sequence, logical time, resource, action, operation, causation,
correlation, and outcome. They never contain the command or resource payload.

## Follow one chain

```bash
simile state inttegro --caused-by request-42
simile state inttegro --correlation checkout-8
simile state inttegro --resource db.orders --after 120 --limit 50
```

`--caused-by` selects direct causal children. `--correlation` selects the broader end-to-end trace.
Both can be combined with a resource and sequence window.

## Inspect values explicitly

```bash
simile state inttegro --resource db.orders --values
```

Stored values appear only with `--values`. This explicit opt-in protects the common diagnostic path
from accidentally printing customer data, mail bodies, object contents, or request-derived state.
JSON output preserves the same boundary.

The command takes the state database's exclusive ownership lock. It therefore refuses to inspect a
file currently owned by a running service rather than racing a writer or reporting a torn view.

## Inspect a running service in Simile Web

Offline inspection and live inspection deliberately use different ownership paths. `simile state`
opens the durable checkpoint when no service owns it. A running service instead exposes its own
in-memory Stack through a read-only loopback inspector, so there is still only one state owner and
the browser cannot observe a torn checkpoint.

The live inspector provides reserved inspection and local-control endpoints:

```text
GET /.simile/inspection
GET /.simile/exchanges
DELETE /.simile/exchanges
GET /.simile/state
POST /.simile/state/reset
GET /.simile/deferred
POST /.simile/exchanges/:id/report
POST /.simile/reports/:id/submit
```

The endpoints are registered only when the service listener is bound to a loopback address. Browser
requests must also come from a loopback origin. State values are present because the Web state view
is an explicit local inspection surface; journal entries remain payload-free. Exchange capture is
bounded to 200 entries, redacts sensitive JSON request fields, omits opaque request payloads, and
does not retain bodies larger than 256 KiB. Destructive controls require explicit confirmation,
and state reset requires a `{"confirm":true}` request body in addition to the browser prompt.

Preparing an exchange report reproduces the captured exchange against the current service artifact
and verifies the resulting evidence locally. Submission remains a separate explicit action, so
local diagnosis never reports data to a configured endpoint merely because an operator opened or
generated a report.

Simile Web polls these endpoints while auto-refresh is enabled. If the runtime is unavailable, it
shows generated preview data and labels that fallback rather than presenting it as observed runtime
evidence. The inspector is not a remote administration API: every endpoint is registered only for
a loopback listener and accepts browser requests only from loopback origins.
