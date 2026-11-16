use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use simile_schema::{
    BodySchemaCompiler, BoundRequestBody, BoundResponseBody, RequestBodyPlan, ResponseBodyPlan,
    SchemaCatalog,
};
use simile_spec::{
    BehaviorOrigin, CompiledService, HttpMethod, OpenApiFeature, ResourceKind, ResourceRef,
    ResponseStatus,
};

use crate::{Error, Result};

pub const FORMAT_VERSION: u16 = 5;
pub const EXTENSION: &str = "similec";

const MAGIC: &[u8; 8] = b"SIMILEC\0";
const FLAGS: u16 = 0;
const DIGEST_LENGTH: usize = 32;
const HEADER_LENGTH: usize = MAGIC.len() + 2 + 2 + 8 + DIGEST_LENGTH;

type StringId = u32;
type ValueId = u32;
type ResourceId = u32;
type AsyncId = u32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    pub format_version: u16,
    pub compiler_version: String,
    pub service: String,
    pub service_version: String,
    pub openapi_version: String,
    pub operations: usize,
    pub request_bodies: usize,
    pub response_bodies: usize,
    pub schema_nodes: usize,
    pub responses: usize,
    pub value_nodes: usize,
    pub resources: usize,
    pub async_operations: usize,
    pub encoded_bytes: usize,
    pub payload_sha256: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Artifact {
    payload: Payload,
    encoded_bytes: usize,
    digest: [u8; DIGEST_LENGTH],
}

/// The runtime-facing portion of one compiled operation.
#[derive(Clone, Copy, Debug)]
pub struct ArtifactOperation<'a> {
    key: &'a str,
    operation_id: Option<&'a str>,
    request_body: Option<&'a RequestBodyPlan>,
    responses: &'a [ResponseRecord],
    behavior_response_status: Option<u16>,
    triggers: &'a [AsyncId],
    strings: &'a [String],
    resources: &'a [ResourceRecord],
    async_operations: &'a [AsyncRecord],
    schemas: &'a SchemaCatalog,
}

/// One compiled response available to the runtime for an operation.
#[derive(Clone, Debug)]
pub struct ArtifactResponse<'a> {
    status: ResponseStatus,
    body: Option<BoundResponseBody<'a>>,
}

/// One asynchronous operation triggered by a compiled behavior.
#[derive(Clone, Copy, Debug)]
pub struct ArtifactAsyncOperation<'a> {
    name: &'a str,
    uses: &'a [ResourceId],
    strings: &'a [String],
    resources: &'a [ResourceRecord],
}

/// One abstract resource used by an asynchronous operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactResource<'a> {
    kind: ResourceKind,
    name: &'a str,
}

impl<'a> ArtifactOperation<'a> {
    #[must_use]
    pub const fn key(&self) -> &'a str {
        self.key
    }

    #[must_use]
    pub const fn operation_id(&self) -> Option<&'a str> {
        self.operation_id
    }

    #[must_use]
    pub fn request_body(&self) -> Option<BoundRequestBody<'a>> {
        self.request_body
            .map(|plan| BoundRequestBody::new(plan, self.schemas))
    }

    pub fn responses(&self) -> impl Iterator<Item = ArtifactResponse<'a>> + 'a {
        self.responses.iter().map(|response| ArtifactResponse {
            status: decode_status(response.0, response.1),
            body: response
                .3
                .as_ref()
                .map(|body| BoundResponseBody::new(body, self.schemas)),
        })
    }

    #[must_use]
    pub const fn behavior_response_status(&self) -> Option<u16> {
        self.behavior_response_status
    }

    /// Return the asynchronous operations attached to this operation's behavior.
    pub fn triggers(&self) -> impl Iterator<Item = ArtifactAsyncOperation<'a>> + 'a {
        let operations = self.async_operations;
        let resources = self.resources;
        let strings = self.strings;
        self.triggers.iter().map(move |id| {
            let operation = &operations[*id as usize];
            ArtifactAsyncOperation {
                name: &strings[operation.0 as usize],
                uses: &operation.1,
                strings,
                resources,
            }
        })
    }
}

impl<'a> ArtifactResponse<'a> {
    #[must_use]
    pub const fn status(&self) -> &ResponseStatus {
        &self.status
    }

    #[must_use]
    pub const fn body(&self) -> Option<BoundResponseBody<'a>> {
        self.body
    }
}

impl<'a> ArtifactAsyncOperation<'a> {
    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// Return the abstract resources declared by this asynchronous operation.
    pub fn resources(&self) -> impl Iterator<Item = ArtifactResource<'a>> + 'a {
        let resources = self.resources;
        let strings = self.strings;
        self.uses.iter().map(move |id| {
            let resource = &resources[*id as usize];
            ArtifactResource {
                kind: decode_resource_kind(resource.0),
                name: &strings[resource.1 as usize],
            }
        })
    }
}

impl<'a> ArtifactResource<'a> {
    #[must_use]
    pub const fn kind(&self) -> ResourceKind {
        self.kind
    }

    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }

    #[must_use]
    pub fn reference(&self) -> ResourceRef {
        ResourceRef::new(self.kind, self.name)
    }
}

impl Artifact {
    #[must_use]
    pub fn summary(&self) -> ArtifactSummary {
        let responses = self
            .payload
            .3
            .iter()
            .map(|operation| operation.7.len())
            .sum();
        ArtifactSummary {
            format_version: FORMAT_VERSION,
            compiler_version: self.string(self.payload.0).to_owned(),
            service: self.string(self.payload.2.0).to_owned(),
            service_version: self.string(self.payload.2.1).to_owned(),
            openapi_version: self.string(self.payload.2.3).to_owned(),
            operations: self.payload.3.len(),
            request_bodies: self
                .payload
                .3
                .iter()
                .filter(|operation| operation.6.is_some())
                .count(),
            response_bodies: self
                .payload
                .3
                .iter()
                .flat_map(|operation| &operation.7)
                .filter(|response| response.3.is_some())
                .count(),
            schema_nodes: self.payload.7.len(),
            responses,
            value_nodes: self.payload.4.len(),
            resources: self.payload.5.len(),
            async_operations: self.payload.6.len(),
            encoded_bytes: self.encoded_bytes,
            payload_sha256: encode_hex(&self.digest),
        }
    }

    /// Find a compiled operation by its stable `METHOD /path` key.
    #[must_use]
    pub fn operation(&self, key: &str) -> Option<ArtifactOperation<'_>> {
        self.payload
            .3
            .binary_search_by(|operation| self.string(operation.0).cmp(key))
            .ok()
            .map(|index| self.operation_at(index))
    }

    /// Find a compiled operation by its exact, case-sensitive OpenAPI `operationId`.
    #[must_use]
    pub fn operation_by_id(&self, operation_id: &str) -> Option<ArtifactOperation<'_>> {
        self.payload
            .3
            .iter()
            .position(|operation| {
                operation
                    .4
                    .is_some_and(|id| self.string(id) == operation_id)
            })
            .map(|index| self.operation_at(index))
    }

    fn operation_at(&self, index: usize) -> ArtifactOperation<'_> {
        let operation = &self.payload.3[index];
        ArtifactOperation {
            key: self.string(operation.0),
            operation_id: operation.4.map(|id| self.string(id)),
            request_body: operation.6.as_ref(),
            responses: &operation.7,
            behavior_response_status: operation.9.as_ref().and_then(|behavior| behavior.0),
            triggers: operation
                .9
                .as_ref()
                .map_or(&[], |behavior| behavior.1.as_slice()),
            strings: &self.payload.1,
            resources: &self.payload.5,
            async_operations: &self.payload.6,
            schemas: &self.payload.7,
        }
    }

    fn string(&self, id: StringId) -> &str {
        // Decoding validates every identifier before constructing Artifact.
        &self.payload.1[id as usize]
    }
}

/// Encode a compiled service into the versioned `.similec` representation.
pub fn encode(service: &CompiledService) -> Result<Vec<u8>> {
    let payload = Payload::compile(service)?;
    let mut body = Vec::new();
    ciborium::into_writer(&payload, &mut body).map_err(|error| Error::Encode(error.to_string()))?;
    let body_length = u64::try_from(body.len()).map_err(|_| Error::TooLarge)?;
    let digest: [u8; DIGEST_LENGTH] = Sha256::digest(&body).into();

    let mut encoded = Vec::with_capacity(HEADER_LENGTH + body.len());
    encoded.extend_from_slice(MAGIC);
    encoded.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    encoded.extend_from_slice(&FLAGS.to_be_bytes());
    encoded.extend_from_slice(&body_length.to_be_bytes());
    encoded.extend_from_slice(&digest);
    encoded.extend_from_slice(&body);
    Ok(encoded)
}

/// Decode and validate a versioned `.similec` artifact.
pub fn decode(encoded: &[u8]) -> Result<Artifact> {
    if encoded.len() < HEADER_LENGTH {
        return Err(Error::Truncated);
    }
    if &encoded[..MAGIC.len()] != MAGIC {
        return Err(Error::InvalidMagic);
    }

    let format = u16::from_be_bytes(read_array(encoded, 8)?);
    if format != FORMAT_VERSION {
        return Err(Error::UnsupportedFormat(format));
    }
    let flags = u16::from_be_bytes(read_array(encoded, 10)?);
    if flags != FLAGS {
        return Err(Error::UnsupportedFlags(flags));
    }
    let body_length = u64::from_be_bytes(read_array(encoded, 12)?);
    let body_length = usize::try_from(body_length).map_err(|_| Error::TooLarge)?;
    let expected_length = HEADER_LENGTH
        .checked_add(body_length)
        .ok_or(Error::TooLarge)?;
    match encoded.len().cmp(&expected_length) {
        std::cmp::Ordering::Less => return Err(Error::Truncated),
        std::cmp::Ordering::Greater => return Err(Error::TrailingBytes),
        std::cmp::Ordering::Equal => {}
    }

    let mut expected_digest = [0_u8; DIGEST_LENGTH];
    expected_digest.copy_from_slice(&encoded[20..HEADER_LENGTH]);
    let body = &encoded[HEADER_LENGTH..];
    let digest: [u8; DIGEST_LENGTH] = Sha256::digest(body).into();
    if digest != expected_digest {
        return Err(Error::Integrity);
    }

    let payload: Payload =
        ciborium::from_reader(body).map_err(|error| Error::Decode(error.to_string()))?;
    payload.validate()?;
    Ok(Artifact {
        payload,
        encoded_bytes: encoded.len(),
        digest,
    })
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset.checked_add(N).ok_or(Error::Truncated)?;
    let source = bytes.get(offset..end).ok_or(Error::Truncated)?;
    let mut value = [0_u8; N];
    value.copy_from_slice(source);
    Ok(value)
}

fn encode_hex(bytes: &[u8]) -> String {
    use fmt::Write;

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

// Tuple records are deliberate: the envelope version owns their field order, and CBOR arrays
// avoid repeating JSON-style field names in every operation and schema node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Payload(
    StringId,
    Vec<String>,
    ServiceRecord,
    Vec<OperationRecord>,
    Vec<ValueRecord>,
    Vec<ResourceRecord>,
    Vec<AsyncRecord>,
    SchemaCatalog,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ServiceRecord(StringId, StringId, Option<StringId>, StringId, u8);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct OperationRecord(
    StringId,
    u8,
    Option<StringId>,
    StringId,
    Option<StringId>,
    Option<StringId>,
    Option<RequestBodyPlan>,
    Vec<ResponseRecord>,
    u8,
    Option<BehaviorRecord>,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ResponseRecord(u8, u16, ValueId, Option<ResponseBodyPlan>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct BehaviorRecord(Option<u16>, Vec<AsyncId>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ResourceRecord(u8, StringId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AsyncRecord(StringId, Vec<ResourceId>);

#[derive(Clone, Debug, PartialEq)]
enum ValueRecord {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    String(StringId),
    Array(Vec<ValueId>),
    Object(Vec<(StringId, ValueId)>),
}

impl Serialize for ValueRecord {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple =
            serializer.serialize_tuple(if matches!(self, Self::Null) { 1 } else { 2 })?;
        match self {
            Self::Null => tuple.serialize_element(&0_u8)?,
            Self::Bool(value) => {
                tuple.serialize_element(&1_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Signed(value) => {
                tuple.serialize_element(&2_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Unsigned(value) => {
                tuple.serialize_element(&3_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Float(value) => {
                tuple.serialize_element(&4_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::String(value) => {
                tuple.serialize_element(&5_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Array(value) => {
                tuple.serialize_element(&6_u8)?;
                tuple.serialize_element(value)?;
            }
            Self::Object(value) => {
                tuple.serialize_element(&7_u8)?;
                tuple.serialize_element(value)?;
            }
        }
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for ValueRecord {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(ValueRecordVisitor)
    }
}

struct ValueRecordVisitor;

impl<'de> Visitor<'de> for ValueRecordVisitor {
    type Value = ValueRecord;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a tagged compiled value node")
    }

    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        use serde::de::Error as _;

        let tag = sequence
            .next_element::<u8>()?
            .ok_or_else(|| A::Error::custom("value node is missing its tag"))?;
        let value = match tag {
            0 => ValueRecord::Null,
            1 => ValueRecord::Bool(required(&mut sequence, "boolean")?),
            2 => ValueRecord::Signed(required(&mut sequence, "signed integer")?),
            3 => ValueRecord::Unsigned(required(&mut sequence, "unsigned integer")?),
            4 => ValueRecord::Float(required(&mut sequence, "floating point number")?),
            5 => ValueRecord::String(required(&mut sequence, "string identifier")?),
            6 => ValueRecord::Array(required(&mut sequence, "array children")?),
            7 => ValueRecord::Object(required(&mut sequence, "object fields")?),
            _ => return Err(A::Error::custom("value node has an unknown tag")),
        };
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(A::Error::custom("value node contains trailing fields"));
        }
        Ok(value)
    }
}

fn required<'de, A, T>(sequence: &mut A, name: &str) -> std::result::Result<T, A::Error>
where
    A: SeqAccess<'de>,
    T: Deserialize<'de>,
{
    use serde::de::Error as _;

    sequence
        .next_element()?
        .ok_or_else(|| A::Error::custom(format!("value node is missing its {name}")))
}

impl Payload {
    fn compile(service: &CompiledService) -> Result<Self> {
        let strings = StringTable::compile(service)?;
        let resource_ids = index_resources(service)?;
        let async_ids = index_async_operations(service)?;
        let mut values = ValueArena::default();
        let mut schema_compiler =
            BodySchemaCompiler::new(&service.contract.schemas, service.contract.openapi.feature);

        let service_record = ServiceRecord(
            strings.id(&service.contract.info.title)?,
            strings.id(&service.contract.info.version)?,
            strings.optional(service.contract.info.description.as_deref())?,
            strings.id(&service.contract.openapi.exact)?,
            encode_openapi_feature(service.contract.openapi.feature),
        );

        let mut operations = Vec::with_capacity(service.operations.len());
        for (key, compiled) in &service.operations {
            let operation = &compiled.contract;
            let (method, additional_method) = encode_method(&operation.method, &strings)?;
            let request_body = operation
                .request
                .as_ref()
                .map(|body| schema_compiler.compile_request(body))
                .transpose()?;
            let mut ordered_responses = operation.responses.iter().collect::<Vec<_>>();
            ordered_responses.sort_by(|left, right| left.status.cmp(&right.status));
            let responses = ordered_responses
                .into_iter()
                .map(|response| {
                    let (kind, status) = encode_status(&response.status);
                    Ok(ResponseRecord(
                        kind,
                        status,
                        values.push(&response.definition, &strings)?,
                        response
                            .body
                            .as_ref()
                            .map(|body| schema_compiler.compile_response(body))
                            .transpose()?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let behavior = compiled
                .behavior
                .as_ref()
                .map(|behavior| {
                    let triggers = behavior
                        .triggers
                        .iter()
                        .map(|name| {
                            async_ids
                                .get(name)
                                .copied()
                                .ok_or_else(|| Error::UnknownReference {
                                    kind: "async operation",
                                    name: name.clone(),
                                })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok::<_, Error>(BehaviorRecord(behavior.response_status, triggers))
                })
                .transpose()?;
            operations.push(OperationRecord(
                strings.id(key.as_str())?,
                method,
                additional_method,
                strings.id(&operation.path)?,
                strings.optional(operation.operation_id.as_deref())?,
                strings.optional(operation.summary.as_deref())?,
                request_body,
                responses,
                encode_origin(compiled.origin),
                behavior,
            ));
        }

        let resources = service
            .resources
            .values()
            .map(|resource| {
                Ok(ResourceRecord(
                    encode_resource_kind(resource.kind),
                    strings.id(&resource.name)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let async_operations = service
            .async_operations
            .values()
            .map(|operation| {
                let uses = operation
                    .uses
                    .iter()
                    .map(|reference| {
                        resource_ids.get(reference).copied().ok_or_else(|| {
                            Error::UnknownReference {
                                kind: "resource",
                                name: reference.as_str().to_owned(),
                            }
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(AsyncRecord(strings.id(&operation.name)?, uses))
            })
            .collect::<Result<Vec<_>>>()?;

        let schemas = schema_compiler.finish()?;

        Ok(Self(
            strings.id(env!("CARGO_PKG_VERSION"))?,
            strings.values,
            service_record,
            operations,
            values.records,
            resources,
            async_operations,
            schemas,
        ))
    }

    fn validate(&self) -> Result<()> {
        let strings = self.1.len();
        let values = self.4.len();
        let resources = self.5.len();
        let async_operations = self.6.len();
        validate_index(self.0, strings, "compiler version")?;
        validate_index(self.2.0, strings, "service title")?;
        validate_index(self.2.1, strings, "service version")?;
        validate_optional_index(self.2.2, strings, "service description")?;
        validate_index(self.2.3, strings, "OpenAPI version")?;
        if self.2.4 > 2 {
            return Err(Error::Invalid("OpenAPI feature line"));
        }

        for operation in &self.3 {
            validate_index(operation.0, strings, "operation key")?;
            if operation.1 > 9 {
                return Err(Error::Invalid("HTTP method"));
            }
            if operation.1 == 9 && operation.2.is_none() {
                return Err(Error::Invalid("additional HTTP method"));
            }
            if operation.1 != 9 && operation.2.is_some() {
                return Err(Error::Invalid("HTTP method payload"));
            }
            validate_optional_index(operation.2, strings, "additional HTTP method")?;
            validate_index(operation.3, strings, "operation path")?;
            validate_optional_index(operation.4, strings, "operation id")?;
            validate_optional_index(operation.5, strings, "operation summary")?;
            if operation
                .6
                .as_ref()
                .is_some_and(|request| !BoundRequestBody::new(request, &self.7).is_well_formed())
            {
                return Err(Error::Invalid("request schema"));
            }
            if operation.8 > 1 {
                return Err(Error::Invalid("behavior origin"));
            }
            for response in &operation.7 {
                if response.0 > 2 {
                    return Err(Error::Invalid("response status"));
                }
                if response.0 == 2 && response.1 != 0 {
                    return Err(Error::Invalid("default response status"));
                }
                validate_index(response.2, values, "response definition")?;
                if response
                    .3
                    .as_ref()
                    .is_some_and(|body| !BoundResponseBody::new(body, &self.7).is_well_formed())
                {
                    return Err(Error::Invalid("response schema"));
                }
            }
            if let Some(behavior) = &operation.9 {
                for trigger in &behavior.1 {
                    validate_index(*trigger, async_operations, "async operation")?;
                }
            }
        }
        for operations in self.3.windows(2) {
            let left = &self.1[operations[0].0 as usize];
            let right = &self.1[operations[1].0 as usize];
            if left >= right {
                return Err(Error::Invalid("operation ordering"));
            }
        }
        for value in &self.4 {
            match value {
                ValueRecord::String(id) => validate_index(*id, strings, "string value")?,
                ValueRecord::Array(children) => {
                    for child in children {
                        validate_index(*child, values, "array child")?;
                    }
                }
                ValueRecord::Object(fields) => {
                    for (key, child) in fields {
                        validate_index(*key, strings, "object key")?;
                        validate_index(*child, values, "object value")?;
                    }
                }
                ValueRecord::Null
                | ValueRecord::Bool(_)
                | ValueRecord::Signed(_)
                | ValueRecord::Unsigned(_)
                | ValueRecord::Float(_) => {}
            }
        }
        for resource in &self.5 {
            if resource.0 > 7 {
                return Err(Error::Invalid("resource kind"));
            }
            validate_index(resource.1, strings, "resource name")?;
        }
        for operation in &self.6 {
            validate_index(operation.0, strings, "async operation name")?;
            for resource in &operation.1 {
                validate_index(*resource, resources, "resource")?;
            }
        }
        if !self.7.is_well_formed() {
            return Err(Error::Invalid("body schema catalog"));
        }
        Ok(())
    }
}

struct StringTable {
    values: Vec<String>,
    ids: BTreeMap<String, StringId>,
}

impl StringTable {
    fn compile(service: &CompiledService) -> Result<Self> {
        let mut values = BTreeSet::new();
        values.insert(env!("CARGO_PKG_VERSION").to_owned());
        values.insert(service.contract.info.title.clone());
        values.insert(service.contract.info.version.clone());
        values.insert(service.contract.openapi.exact.clone());
        if let Some(description) = &service.contract.info.description {
            values.insert(description.clone());
        }
        for (key, compiled) in &service.operations {
            values.insert(key.as_str().to_owned());
            let operation = &compiled.contract;
            values.insert(operation.path.clone());
            if let HttpMethod::Additional(method) = &operation.method {
                values.insert(method.clone());
            }
            if let Some(operation_id) = &operation.operation_id {
                values.insert(operation_id.clone());
            }
            if let Some(summary) = &operation.summary {
                values.insert(summary.clone());
            }
            for response in &operation.responses {
                collect_value_strings(&response.definition, &mut values);
            }
            if let Some(behavior) = &compiled.behavior {
                values.insert(behavior.operation_id.clone());
                values.extend(behavior.triggers.iter().cloned());
            }
        }
        for resource in service.resources.values() {
            values.insert(resource.name.clone());
        }
        for operation in service.async_operations.values() {
            values.insert(operation.name.clone());
        }

        let values = values.into_iter().collect::<Vec<_>>();
        let ids = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let id = u32::try_from(index).map_err(|_| Error::Limit("strings"))?;
                Ok((value.clone(), id))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Self { values, ids })
    }

    fn id(&self, value: &str) -> Result<StringId> {
        self.ids
            .get(value)
            .copied()
            .ok_or(Error::Invalid("string table"))
    }

    fn optional(&self, value: Option<&str>) -> Result<Option<StringId>> {
        value.map(|value| self.id(value)).transpose()
    }
}

#[derive(Default)]
struct ValueArena {
    records: Vec<ValueRecord>,
}

impl ValueArena {
    fn push(&mut self, value: &Value, strings: &StringTable) -> Result<ValueId> {
        let record = match value {
            Value::Null => ValueRecord::Null,
            Value::Bool(value) => ValueRecord::Bool(*value),
            Value::Number(value) => {
                if let Some(value) = value.as_i64() {
                    ValueRecord::Signed(value)
                } else if let Some(value) = value.as_u64() {
                    ValueRecord::Unsigned(value)
                } else {
                    ValueRecord::Float(value.as_f64().ok_or(Error::Invalid("number"))?)
                }
            }
            Value::String(value) => ValueRecord::String(strings.id(value)?),
            Value::Array(values) => {
                let children = values
                    .iter()
                    .map(|value| self.push(value, strings))
                    .collect::<Result<Vec<_>>>()?;
                ValueRecord::Array(children)
            }
            Value::Object(values) => {
                let mut ordered = values.iter().collect::<Vec<_>>();
                ordered.sort_by(|(left, _), (right, _)| left.cmp(right));
                let fields = ordered
                    .into_iter()
                    .map(|(key, value)| Ok((strings.id(key)?, self.push(value, strings)?)))
                    .collect::<Result<Vec<_>>>()?;
                ValueRecord::Object(fields)
            }
        };
        let id = u32::try_from(self.records.len()).map_err(|_| Error::Limit("value nodes"))?;
        self.records.push(record);
        Ok(id)
    }
}

fn collect_value_strings(value: &Value, strings: &mut BTreeSet<String>) {
    match value {
        Value::String(value) => {
            strings.insert(value.clone());
        }
        Value::Array(values) => {
            for value in values {
                collect_value_strings(value, strings);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                strings.insert(key.clone());
                collect_value_strings(value, strings);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn index_resources(service: &CompiledService) -> Result<BTreeMap<ResourceRef, ResourceId>> {
    service
        .resources
        .keys()
        .enumerate()
        .map(|(index, reference)| {
            let id = u32::try_from(index).map_err(|_| Error::Limit("resources"))?;
            Ok((reference.clone(), id))
        })
        .collect()
}

fn index_async_operations(service: &CompiledService) -> Result<BTreeMap<String, AsyncId>> {
    service
        .async_operations
        .keys()
        .enumerate()
        .map(|(index, name)| {
            let id = u32::try_from(index).map_err(|_| Error::Limit("async operations"))?;
            Ok((name.clone(), id))
        })
        .collect()
}

fn encode_method(method: &HttpMethod, strings: &StringTable) -> Result<(u8, Option<StringId>)> {
    Ok(match method {
        HttpMethod::Get => (0, None),
        HttpMethod::Put => (1, None),
        HttpMethod::Post => (2, None),
        HttpMethod::Delete => (3, None),
        HttpMethod::Options => (4, None),
        HttpMethod::Head => (5, None),
        HttpMethod::Patch => (6, None),
        HttpMethod::Trace => (7, None),
        HttpMethod::Query => (8, None),
        HttpMethod::Additional(method) => (9, Some(strings.id(method)?)),
    })
}

const fn encode_openapi_feature(feature: OpenApiFeature) -> u8 {
    match feature {
        OpenApiFeature::V3_0 => 0,
        OpenApiFeature::V3_1 => 1,
        OpenApiFeature::V3_2 => 2,
    }
}

const fn encode_status(status: &ResponseStatus) -> (u8, u16) {
    match status {
        ResponseStatus::Exact(status) => (0, *status),
        ResponseStatus::Range(status) => (1, *status),
        ResponseStatus::Default => (2, 0),
    }
}

fn decode_status(kind: u8, status: u16) -> ResponseStatus {
    match kind {
        0 => ResponseStatus::Exact(status),
        1 => ResponseStatus::Range(status),
        2 => ResponseStatus::Default,
        _ => unreachable!("artifact validation rejects unknown response status kinds"),
    }
}

const fn encode_origin(origin: BehaviorOrigin) -> u8 {
    match origin {
        BehaviorOrigin::OpenApi => 0,
        BehaviorOrigin::Hcl => 1,
    }
}

const fn encode_resource_kind(kind: ResourceKind) -> u8 {
    match kind {
        ResourceKind::Db => 0,
        ResourceKind::Cache => 1,
        ResourceKind::Queue => 2,
        ResourceKind::Mail => 3,
        ResourceKind::ObjectStore => 4,
        ResourceKind::EventBus => 5,
        ResourceKind::Scheduler => 6,
        ResourceKind::Http => 7,
    }
}

fn decode_resource_kind(kind: u8) -> ResourceKind {
    match kind {
        0 => ResourceKind::Db,
        1 => ResourceKind::Cache,
        2 => ResourceKind::Queue,
        3 => ResourceKind::Mail,
        4 => ResourceKind::ObjectStore,
        5 => ResourceKind::EventBus,
        6 => ResourceKind::Scheduler,
        7 => ResourceKind::Http,
        _ => unreachable!("artifact validation rejects unknown resource kinds"),
    }
}

fn validate_index(id: u32, length: usize, field: &'static str) -> Result<()> {
    if usize::try_from(id).map_or(true, |id| id >= length) {
        return Err(Error::Invalid(field));
    }
    Ok(())
}

fn validate_optional_index(id: Option<u32>, length: usize, field: &'static str) -> Result<()> {
    if let Some(id) = id {
        validate_index(id, length, field)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_dangling_indexes() {
        let payload = Payload(
            0,
            vec![
                "0.1.0".to_owned(),
                "Orders".to_owned(),
                "1.0.0".to_owned(),
                "3.2.1".to_owned(),
                "/orders".to_owned(),
            ],
            ServiceRecord(1, 2, None, 3, 2),
            vec![OperationRecord(
                99,
                2,
                None,
                4,
                None,
                None,
                None,
                Vec::new(),
                0,
                None,
            )],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            SchemaCatalog::default(),
        );

        assert!(matches!(
            payload.validate(),
            Err(Error::Invalid("operation key"))
        ));
    }
}
