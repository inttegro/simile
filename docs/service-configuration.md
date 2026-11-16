# Service configuration

A service configuration is the composition root for one locally runnable emulator. It names the
service, selects one OpenAPI source, defines its listener, and optionally imports or contains HCL
behavior declarations. `simile start` validates and compiles the complete configuration before it
binds a socket.

```hcl
service "inttegro" {
  listen {
    host = "127.0.0.1"
    port = 4242
  }

  openapi {
    path = "./commerce.yml"
  }

  behaviors = [
    "./orders.hcl",
    "./payments.hcl",
  ]
}
```

The document must contain exactly one `service` block. Its label is the service name passed to the
CLI:

```console
$ simile start inttegro --config ./inttegro.simile.hcl
$ simile start inttegro --config ./inttegro.simile.hcl --port 8484
```

`--port` has invocation precedence over `listen.port`; it does not edit the configuration.

## Listener

`listen.port` is required and must be an integer from 1 through 65535. `listen.host` is optional
and defaults to the loopback address `127.0.0.1`. The host must currently be an IP address rather
than a DNS name. Binding a public interface is therefore a deliberate configuration choice, not a
CLI default.

## OpenAPI source

The `openapi` block must contain exactly one source:

```hcl
openapi { path = "./commerce.yml" }
```

or:

```hcl
openapi { url = "https://example.com/commerce.yml" }
```

A relative path is resolved from the directory containing the service configuration. A URL is read
into memory for compilation and is not saved to disk.

## Behavior modules

`behaviors` is an optional list of HCL module paths. Relative paths resolve from the configuration
directory. The service document may also contain the existing `resource`, `async`, and `behavior`
blocks after the `service` block:

```hcl
resource "queue" "orders" {}

async "dispatch_order" {
  uses = ["queue.orders"]
}

behavior "createOrder" {
  response { status = 201 }
  trigger "dispatch_order" {}
}
```

Resources are the portable dependencies of the emulated service. They name abstract capabilities
such as `db`, `cache`, `queue`, `mail`, `object_store`, `event_bus`, `scheduler`, or `http`, never a
production vendor. Async declarations connect observable follow-up work to those capabilities.

Imported and inline declarations enter the same semantic compiler. Duplicate declarations,
unknown OpenAPI operation IDs, undeclared resources, and contradictory response selections fail
startup before the listener binds.

## Current HTTP boundary

The server compiles OpenAPI routes into a route table and delegates matched operations to the
protocol-neutral runtime. It currently provides:

- literal and `{parameter}` path-template matching, with literal routes taking precedence;
- `404 Not Found` and `405 Method Not Allowed` responses at the transport boundary;
- an 8 MiB request-body limit;
- JSON request-body decoding and compiled-schema validation through the runtime;
- generated OpenAPI response status, media type, and body;
- process-local replay for `Idempotency-Key` and for the full URI of GET requests; and
- one-line access logs containing method, path, status, request content type and size, response
  content type and size, and elapsed microseconds; and
- graceful shutdown on Ctrl-C.

Access logs are emitted to stderr at `info` level by default for `simile start`. Pass `--quiet` to
disable them, or use `SIMILE_LOG` to select a tracing filter explicitly. Request and response
bodies, query strings, credentials, idempotency keys, and other header values are never written to
the access log.

Path, query, and header schema validation are not yet compiled into the runtime artifact. Form and
multipart codecs are also not yet implemented. HCL-declared asynchronous operations are returned
by the runtime as plans, but the server does not yet execute abstract resources or persist durable
service state. These omissions are reported here so that starting a service is not mistaken for
full behavioral fidelity.
