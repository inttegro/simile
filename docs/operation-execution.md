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

The normal execution path selects its response from the compiled OpenAPI contract and optional HCL
behavior. Test scenarios that need a particular declared response can use
`Runtime::execute_with_options` with separate `ExecutionOptions`; those controls are not presented
as part of the incoming operation call.

It performs one ordered run:

1. resolve the operation from the loaded artifact;
2. decode and validate the request body;
3. synthesize the immediate response, using the validated request as context; and
4. collect the asynchronous operations attached by HCL, including their abstract resources.

The result owns the validated request body, synthesized response, and asynchronous operation plan.
No plan is returned when validation or response synthesis fails. This gives future state and effect
execution a single success boundary instead of spreading partial work across transport handlers.

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
4. call `Runtime::execute`; and
5. translate the result or runtime error into an HTTP response.

Route-template matching, parameters, headers, authentication, and request-size enforcement belong
to that HTTP boundary or to later runtime primitives. They are not silently treated as implemented
by the current body-oriented runtime.
