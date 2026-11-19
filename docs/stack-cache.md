---
title: Cache
description: Deterministic expiry, counters, deletion, and compare-and-set semantics.
---

`simile-cache` models volatile named values whose lifetime and atomic updates affect observable
service behavior.

## Capabilities

- get, set, and delete
- optional millisecond TTL on stored values
- atomic signed-integer increment
- compare-and-set using monotonically increasing entry versions

Expiry uses [logical time](/stack-time). An entry is absent at the exact instant its expiry is
reached, whether it is removed by a clock advance or encountered by a later command.

Compare-and-set fails with `conflict` if the expected version no longer matches. Increment fails
with `invalid_value` when the current value is not an integer or the calculation overflows.

## Boundary

The cache does not emulate Redis commands, Memcached wire behavior, eviction pressure, clustering,
or vendor-specific data structures. Model only the cache semantics that change the public service
behavior.
