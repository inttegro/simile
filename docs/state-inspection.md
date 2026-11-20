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
