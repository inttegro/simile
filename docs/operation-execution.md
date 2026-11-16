# Operation execution

Simile executes a compiled service through a protocol-neutral runtime. The runtime does
not open a port, route a URL, or depend on an HTTP framework. It accepts the stable operation key
selected by a caller and coordinates the runtime primitives that already understand the compiled
artifact.

## Execution boundary

`Runtime::execute` accepts an `OperationCall` containing:

- a stable operation key such as `POST /orders`;
- the incoming request Content-Type; and
- the incoming body bytes.

Transport adapters that have already decoded a body call `Runtime::execute_value` with an
`OperationValueCall`. This is the boundary used by the one-shot CLI: JSON, form, and future wire
formats remain adapter concerns while schema validation and response projection operate on one
normalized value model.

The normal execution path selects its response from the compiled OpenAPI contract and optional HCL
behavior. Test scenarios that need a particular declared response can use
`Runtime::execute_with_options` with separate `ExecutionOptions`; those controls are not presented
as part of the incoming operation call. An adapter can also supply an opaque `ReplayIdentity`:
`IdempotencyKey` for mutations or `Resource` for GET requests.

It performs one ordered run:

1. resolve the operation from the loaded artifact;
2. decode and validate the request body;
3. synthesize the immediate response, using the validated request as context; and
4. collect the asynchronous operations attached by HCL, including their abstract resources; and
5. cache the completed outcome when the adapter supplied a replay identity.

The result owns the validated request body, synthesized response, and asynchronous operation plan.
No plan is returned when validation or response synthesis fails. This gives future state and effect
execution a single success boundary instead of spreading partial work across transport handlers.

## One-shot CLI execution

The `simile run` command compiles a local OpenAPI path or an in-memory `--spec-url` response and
invokes this runtime boundary once. Callers select the operation using either its stable
`METHOD /path` key or its exact OpenAPI `operationId`. The selectors are mutually exclusive.

If the caller supplies `--request`, Simile reads an inline JSON request object, a bare file path
(`@file` remains supported), or standard input when passed `-`. The object contains a `headers`
object and an optional `body`. Header names are matched case-insensitively. `Content-Type` selects
the OpenAPI media type used to validate a provided body; the remaining headers are retained in the
execution report but are not interpreted as runtime policy. Supplying two case variants of
`Content-Type` is an error. A present JSON `null` body is distinct from an omitted body.

If any request body is declared and `--request` is absent, Simile synthesizes and validates a fresh
value from the compiled schema. This also applies to optional request bodies; only an
operation with no request-body declaration runs without one by default. `--content-type` selects a
media type for synthesis and cannot be combined with `--request`. By default, stdout is only the
pretty-printed JSON response payload; a bodyless response is `null`. Passing `--verbose` prints the
selected operation, normalized request, provided headers, response metadata, and asynchronous
plan. Each invocation is a fresh process and therefore produces fresh schema-valid values; it is
not an HTTP server or a persistent replay store.

## Replay identity

A `Runtime` owns an in-memory replay index for its lifetime. A repeated idempotency key returns the
first completed outcome, including its generated response and asynchronous plan. Reusing that key
with a different validated request is an error. A repeated `Resource` identity is accepted only for
a GET operation and returns the first outcome for that operation and resource. Different identities,
or calls without an identity, generate fresh outcomes.

The runtime deliberately does not inspect headers, route templates, or parameter names. A future
HTTP adapter must normalize the idempotency header or addressed resource into `ReplayIdentity`.
This keeps HTTP policy out of the schema and operation layers, and prevents route names from being
mistaken for storage semantics.

## Asynchronous work

The runtime plans declared work; it does not claim that the work has executed. Each planned entry
contains the HCL async-operation name and its abstract resource references, such as `db.orders` or
`mail.receipts`. A subsequent runtime layer can dispatch those entries to deterministic local
resource implementations and record their progress without reparsing HCL or performing string
lookups through the compiled service graph.

OpenAPI-only operations produce an empty asynchronous plan. Simile does not infer storage or
effects from route names.

## Future HTTP adapter

The server layer remains deliberately absent. When it is added, it should remain a wire adapter:

1. listen on the configured local port;
2. match the incoming method and path to a compiled operation key;
3. apply transport limits and gather the request values;
4. decode the wire body into a normalized value and call `Runtime::execute_value`; and
5. translate the result or runtime error into an HTTP response.

Route-template matching, parameters, headers, authentication, and request-size enforcement belong
to that HTTP boundary or to later runtime primitives. They are not silently treated as implemented
by the current body-oriented runtime.
