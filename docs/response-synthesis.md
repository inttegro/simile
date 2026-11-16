# Response synthesis

Simile synthesizes responses from the compiled `.similec` artifact. It does not parse OpenAPI or
HCL at runtime, and it does not require an incoming request to produce a schema-valid response.

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

Form, multipart, and non-JSON response encoders are outside the current slice. Declared non-JSON
content produces an explicit unsupported-encoder error rather than unverified bytes.
