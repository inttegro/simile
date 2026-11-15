# Compiled artifact format

`.similec` is Simile's runtime distribution format. OpenAPI and HCL remain the provider-authored
and reviewable source formats; the runtime never needs to parse either one on startup.

## Envelope version 2

Every artifact begins with a fixed 52-byte envelope:

| Offset | Size | Value |
| ---: | ---: | --- |
| 0 | 8 | ASCII `SIMILEC` followed by NUL |
| 8 | 2 | unsigned big-endian format version |
| 10 | 2 | unsigned big-endian flags; zero in version 2 |
| 12 | 8 | unsigned big-endian payload length |
| 20 | 32 | SHA-256 digest of the payload |
| 52 | variable | deterministic CBOR payload |

Readers must reject an unknown format version or non-zero flags. They must also reject a truncated
payload, trailing bytes, a digest mismatch, unknown record tags, and every out-of-range internal
index. These checks happen before runtime execution.

## Payload model

The payload is an ordered CBOR array rather than a serialized Rust struct map. Version 2 contains:

1. compiler version string identifier;
2. a sorted and deduplicated string table;
3. service metadata;
4. operations in stable operation-key order;
5. an indexed value arena used by response definitions;
6. resources in stable resource-reference order; and
7. asynchronous operations in stable name order; and
8. one shared request-schema catalog.

Strings, value nodes, resources, and asynchronous operations are referenced with unsigned numeric
identifiers. Behavior records therefore point directly to asynchronous-operation records, and
those records point directly to abstract resources. The runtime does not repeatedly compare or
allocate names to follow those edges.

Each operation embeds a compact request-body plan containing its declared media types and schema
root identifiers. Those roots point into the shared catalog, where component schemas are compiled
once and recursive references remain graph edges. The catalog contains only the constraints needed
by request validation. The runtime therefore validates a request without parsing OpenAPI,
interpreting arbitrary source JSON, or copying common schemas into every operation.

JSON values retained from an OpenAPI source are lowered into tagged value nodes: null, boolean,
signed integer, unsigned integer, floating point, string identifier, child-node array, or sorted
object fields. The original JSON text and object-key repetition are not stored in the artifact.

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
