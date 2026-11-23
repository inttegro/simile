# Simile Web interface study

This browser-native interface inspects a local Simile service. It is intentionally buildless so the
execution hierarchy, density, responsiveness, URL routing, and interaction model can be evaluated
without introducing a frontend toolchain.

The Inttegro service view is generated from the real public Commerce contract and Simile
configuration. Rebuild it after either source changes:

```bash
node scripts/build-inttegro-service.mjs
```

The generator compiles `openapi/simile/Simfile`, inspects the resulting artifact, runs the fidelity
audit, and combines that evidence with `openapi/commerce.yml`. The browser therefore renders the
real operations, outcomes, scenarios, backends, profiles, current gaps, and a read-only source
snapshot of the Simfile and its referenced HCL modules in Settings.

Start the service and this directory on loopback:

```bash
SIMILE_HOME=/tmp/simile-web-live ./target/debug/simile start --config ../openapi/simile/Simfile
python3 -m http.server 4175 --bind 127.0.0.1 --directory web
```

Open `http://127.0.0.1:4175/`. The interface polls the running service's read-only inspector and
switches Exchanges and State from clearly labelled preview data to live runtime data. It uses:

- `GET /.simile/inspection` for service and capture counts;
- `GET /.simile/exchanges` for a bounded, newest-first exchange buffer; and
- `GET /.simile/state` for the running Stack's resources, values, and payload-free journal.

These endpoints exist only when the emulated service is bound to a loopback address. Browser access
is restricted to loopback origins. Captured request JSON passes through the server's sensitive-field
redactor, opaque request bodies are omitted, and request or response bodies above 256 KiB are marked
as truncated. The ring buffer retains at most 200 exchanges in memory and does not alter the durable
checkpoint.
