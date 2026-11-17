---
title: Response synthesis
description: How Simile selects a legal response and constructs a sound payload with or without a request.
---

Simile synthesizes responses from the compiled `.similec` artifact. It does not parse OpenAPI or
HCL at runtime, and it does not require an incoming request to produce a contract-sound response.

There are two separate questions: **which declared response should be used?** and **what valid body
should that response contain?** Selection is handled before generation so the synthesizer never
chooses an arbitrary error or success family.

## Selection

The runtime locates an operation by its stable key, such as `POST /orders`, then chooses a response
in this order:

1. a status explicitly supplied by the caller;
2. the response status selected by a compiled HCL behavior;
3. the lowest exact successful status;
4. a declared `2XX` range, represented as status 200;
5. the lowest other exact status;
6. the lowest declared status range; or
7. a `default` response, represented as status 500.

For an explicit or HCL-selected status, an exact response wins over a matching range, which wins
over `default`. OpenAPI does not give a `default` response a concrete wire status, so Simile uses
500 when it is the only spec-derived choice; an explicit caller or behavior status is preserved
instead. A response without content remains bodyless; Simile does not invent a media type or
payload.

## Body generation

For a response with content, Simile selects a declared JSON media type unless the caller asks for
one explicitly. Fresh synthesis varies unconstrained values instead of pinning every call to a
published example or default. Constants remain fixed and enums stay within their declared values.
The candidate is always validated again before it is encoded.

That final validation includes compiled cross-field relationships. A response is returned only when
the complete candidate is sound under both the supported OpenAPI schema rules and every recognized
`x-simile-constraints` declaration. This includes typed ordering, relative bounds, implication,
equality, equation, aggregate, distinct-selector, scalar-set, template, and lifecycle proofs. See
[payload soundness](payload-soundness.md).

Explicit OpenAPI `format` annotations are compiled with the schema. Simile emits varying
format-valid values for supported standard formats and the provider formats it understands, while
JSON integers remain integer-shaped on the wire. It does not infer formats from descriptions. A
seeded synthesis entry point reproduces a generated response when a test needs exact replay.

This spec-only path is the baseline and works without request context.

## Request-aware values

When the caller supplies a body already returned by `validate_request_body`, response synthesis may
project values from it. Projection is deliberately structural:

- only fields declared at the same path in the response are considered;
- a value is copied only when the response field schema accepts it;
- nested objects are merged so generated response-only fields survive;
- incompatible request values leave the synthesized baseline unchanged; and
- the complete result must still pass response validation.

This is not global name matching and does not infer private business rules. A field marked
`writeOnly` is omitted while the response graph is compiled, so credentials and other request-only
data cannot be reflected through this mechanism. A property marked `readOnly` remains available to
response synthesis.

For example, if both request and response declare compatible `/product` and `/quantity` fields,
those values can be copied into the generated response. A generated `/id` remains intact. A
request-only secret marked `writeOnly` never enters the response graph.

Form, multipart, and non-JSON response encoders are outside the current slice. Declared non-JSON
content produces an explicit unsupported-encoder error rather than unverified bytes.

Read [Outcome fidelity](/outcome-fidelity) for named exact results and
[Payload soundness](/payload-soundness) for the final proof gate.
