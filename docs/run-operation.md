---
title: Run one operation
description: Execute one OpenAPI operation directly, with a provided or synthesized request.
---

`simile run` is the shortest path from an OpenAPI operation to an executed result. It compiles the
contract in memory, selects one operation, prepares a request, invokes the same protocol-neutral
runtime used by the HTTP server, and exits.

## Select the contract and operation

Use a local specification:

```bash
simile run ./openapi.yaml --operation-id createOrder
```

Or read a remote specification into memory:

```bash
simile run \
  --spec-url https://example.com/openapi.yaml \
  --operation "POST /orders"
```

A positional specification path and `--spec-url` are mutually exclusive. A structural
`--operation "METHOD /path"` selector and an exact, case-sensitive `--operation-id` are also
mutually exclusive. Remote OpenAPI is read with a 64 MiB limit and is not saved to disk.

## Let Simile build the request

When the operation declares any request body and `--request` is absent, Simile chooses a declared
JSON media type, synthesizes a fresh schema-valid value, and validates it through the ordinary
request path. This applies even when OpenAPI marks the body optional. An operation with no request
body runs without one.

Choose a particular declared media type with `--content-type`:

```bash
simile run ./openapi.yaml \
  --operation-id createOrder \
  --content-type application/json
```

`--content-type` is only for synthesis and conflicts with `--request`.

## Provide the request

The request is a complete JSON document with string headers and an optional body:

```json
{
  "headers": {
    "Content-Type": "application/json",
    "Idempotency-Key": "order-123"
  },
  "body": {
    "product": "custom_jingle",
    "quantity": 1
  }
}
```

Pass it inline, as a file path, or through standard input:

```bash
simile run ./openapi.yaml --operation-id createOrder --request request.json
cat request.json | simile run ./openapi.yaml --operation-id createOrder --request -
```

Header names are matched case-insensitively. `Content-Type` selects validation for a provided body;
two case variants of that header are rejected. An omitted `body` is different from an explicit
JSON `null` body.

The one-shot command retains other headers in its verbose report but does not apply HTTP adapter
policy to them. In particular, it does not create persistent replay across separate invocations.

## Choose an outcome

An HCL overlay can publish named outcomes and opaque scenarios:

```bash
simile run ./openapi.yaml \
  --behavior ./payments.hcl \
  --operation-id createPayment \
  --outcome card_declined
```

```bash
simile run ./openapi.yaml \
  --behavior ./payments.hcl \
  --operation-id createPayment \
  --scenario payment_declined
```

`--outcome` and `--scenario` are mutually exclusive. These are test controls outside the emulated
request; applications do not need magic headers to reach them.

## Read the output

Without `--verbose`, stdout is only the response payload as pretty JSON. A bodyless response is
printed as `null`:

```bash
simile run ./openapi.yaml --operation-id createOrder | jq .id
```

With `--verbose`, Simile also reports the normalized request, operation, response metadata, and
selected outcome and provenance. Diagnostics remain on stderr so stdout is safe to pipe.

Next, read [Request validation](/request-validation) and
[Response synthesis](/response-synthesis) for the two halves of execution.
