use simile_spec::{ResourceKind, ResourceRef};

#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub name: &'a str,
    pub contents: &'a str,
}

impl<'a> Source<'a> {
    #[must_use]
    pub const fn new(name: &'a str, contents: &'a str) -> Self {
        Self { name, contents }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDeclaration {
    pub kind: ResourceKind,
    pub name: String,
}

impl ResourceDeclaration {
    #[must_use]
    pub fn reference(&self) -> ResourceRef {
        ResourceRef::new(self.kind, &self.name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BehaviorDeclaration {
    pub operation_id: String,
    pub response_status: Option<u16>,
    pub triggers: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsyncDeclaration {
    pub name: String,
    pub uses: Vec<ResourceRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Module {
    pub source: String,
    pub resources: Vec<ResourceDeclaration>,
    pub behaviors: Vec<BehaviorDeclaration>,
    pub async_operations: Vec<AsyncDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceDocument {
    pub service: ServiceDeclaration,
    pub inline_behavior: Module,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceDeclaration {
    pub name: String,
    pub listen: ListenDeclaration,
    pub openapi: OpenApiDeclaration,
    pub behaviors: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListenDeclaration {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenApiDeclaration {
    Path(String),
    Url(String),
}
