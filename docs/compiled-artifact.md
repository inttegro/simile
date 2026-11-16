# Compiled artifact format

`.similec` is Simile's runtime distribution format. OpenAPI and HCL remain the provider-authored
and reviewable source formats; the runtime never needs to parse either one on startup.

## Envelope version 8

Every artifact begins with a fixed 52-byte envelope:

| Offset | Size | Value |
| ---: | ---: | --- |
| 0 | 8 | ASCII `SIMILEC` followed by NUL |
| 8 | 2 | unsigned big-endian format version |
| 10 | 2 | unsigned big-endian flags; zero in version 8 |
| 12 | 8 | unsigned big-endian payload length |
| 20 | 32 | SHA-256 digest of the payload |
| 52 | variable | deterministic CBOR payload |

Readers must reject an unknown format version or non-zero flags. They must also reject a truncated
payload, trailing bytes, a digest mismatch, unknown record tags, and every out-of-range internal
index. These checks happen before runtime execution.

## Payload model

The payload is an ordered CBOR array rather than a serialized Rust struct map. Version 8 contains:

1. compiler version string identifier;
2. a sorted and deduplicated string table;
3. service metadata;
4. operations in stable operation-key order;
5. an indexed value arena used by response definitions;
6. resources in stable resource-reference order;
7. asynchronous operations in stable name order; and
8. one shared request/response schema catalog.

Strings, value nodes, resources, and asynchronous operations are referenced with unsigned numeric
identifiers. Behavior records therefore point directly to asynchronous-operation records, and
those records point directly to abstract resources. The runtime does not repeatedly compare or
allocate names to follow those edges.

Each operation embeds a compact request-body plan and a body plan for every response that declares
content. Plans contain declared media types, schema root identifiers, and optional declared media
examples. Those roots point into the shared catalog, where component schemas are compiled once per
request/response direction and recursive references remain graph edges. Direction matters:
`readOnly` properties are omitted from request graphs and `writeOnly` properties are omitted from
response graphs. The catalog also contains compact schema example and default hints, plus explicit
OpenAPI `format` annotations used by seeded synthesis. Description prose is deliberately not
interpreted as schema metadata. The runtime therefore validates requests and synthesizes requests
or responses without parsing OpenAPI, interpreting arbitrary source JSON, or copying common
schemas into every operation.

Recognized `x-simile-constraints` declarations are compiled into typed relationship records in the
same schema graph. Source extension objects are not shipped as arbitrary JSON. Artifact validation
rejects relationship selectors, predicates, witnesses, expressions, lifecycle plans, or comparison
kinds that do not match the embedded graph. Version 8 carries ordering, relative-bound,
implication, equality, equation, aggregate, distinct-selector, scalar-set, template, and lifecycle
records; the runtime does not reparse their OpenAPI extension syntax.

Version 8 also carries typed provider-neutral generation plans. Every plan records a stable rule
identifier, confidence, source-field evidence, and a typed repair. The runtime consumes those
plans without re-running name inference. Provider-specific profiles are not part of this version.

Response-definition JSON retained from an OpenAPI source is lowered into tagged value nodes: null,
boolean, signed integer, unsigned integer, floating point, string identifier, child-node array, or
sorted object fields. Schema and media synthesis hints are encoded as CBOR values inside their
catalog records. The original JSON text is never stored in the artifact.

## Determinism and compatibility

String collection and every map-backed record set are ordered before encoding. CBOR records use
arrays and integer tags, so field names are not repeated. Given the same `CompiledService`, the
encoder must produce byte-identical output; this is covered by tests.

The envelope version is the compatibility boundary. Adding optional source syntax does not require
a format change when it lowers into existing records. A change that alters record meaning or order
requires a new format version and an explicit reader policy. Rust type layout, crate versions, and
Serde defaults are not compatibility promises.

The payload digest protects integrity and enables content-addressed caching. It is not a signature
and does not establish who published an artifact. Artifact signing and provider trust policy belong
to a later distribution layer.
