---
title: Outbound HTTP
description: Match expected service calls, return local responses, and inspect request history.
---

`simile-http` prevents an emulated service from calling real third parties while still letting a
test prove that an outbound integration was invoked correctly.

## Expectations

An expectation defines:

- an identity, method, and exact URL;
- required request headers;
- an optional exact JSON body;
- the local response to return; and
- an optional number of times it may match.

The first matching expectation wins. A matched finite expectation is removed when its remaining
count reaches zero. An unmatched request receives a deterministic `501` response with the public
code `unexpected_request`.

## Request history

Every call is recorded with logical time, sequence, request data, and the matched expectation
identity. Tests can inspect the latest bounded history to prove that a webhook, tax calculation, or
other outbound call was attempted with the intended input.

This component deliberately does not provide general internet access, DNS, TLS, redirects, or a
full HTTP proxy. It emulates the service's declared outbound contract and keeps local tests
hermetic.
