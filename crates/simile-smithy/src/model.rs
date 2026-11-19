use std::collections::BTreeMap;

use serde_json::{Map, Number, Value};
use tree_sitter::Node;

use crate::error::{Error, Result};

#[derive(Clone, Debug, Default)]
pub(crate) struct Traits(BTreeMap<String, Value>);

impl Traits {
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.0.extend(other.0);
    }

    pub(crate) fn insert(&mut self, name: impl Into<String>, value: Value) {
        self.0.insert(name.into(), value);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) target: String,
    pub(crate) traits: Traits,
    start: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum ShapeKind {
    Service {
        version: String,
        operations: Vec<String>,
    },
    Operation {
        input: Option<String>,
        output: Option<String>,
        errors: Vec<String>,
    },
    Structure {
        members: Vec<Member>,
        mixins: Vec<String>,
    },
    Union {
        members: Vec<Member>,
    },
    List {
        member: Member,
    },
    Map {
        value: Member,
    },
    Simple {
        smithy_type: String,
    },
    Enum {
        integer: bool,
        values: Vec<Value>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct Shape {
    pub(crate) name: String,
    pub(crate) traits: Traits,
    pub(crate) kind: ShapeKind,
    start: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Model {
    pub(crate) shapes: BTreeMap<String, Shape>,
}

impl Model {
    pub(crate) fn parse(root: Node<'_>, source: &[u8]) -> Result<Self> {
        let mut statements = Vec::new();
        collect_statements(root, &mut statements);
        let mut model = Self::default();
        let mut applications = Vec::new();

        for statement in statements {
            match statement.kind() {
                "shape_statement" => {
                    let shape = parse_shape(statement, source)?;
                    model.shapes.insert(shape.name.clone(), shape);
                }
                "apply_statement" => applications.push(parse_application(statement, source)?),
                _ => {}
            }
        }

        for (target, traits) in applications {
            let (shape_name, member_name) = target
                .split_once('$')
                .map_or((target.as_str(), None), |(shape, member)| {
                    (shape, Some(member))
                });
            let Some(shape) = model.shapes.get_mut(shape_name) else {
                return Err(Error::UnknownShape(shape_name.to_owned()));
            };
            match member_name {
                None => shape.traits.extend(traits),
                Some(member_name) => {
                    let members = match &mut shape.kind {
                        ShapeKind::Structure { members, .. } | ShapeKind::Union { members } => {
                            members
                        }
                        _ => continue,
                    };
                    if let Some(member) = members.iter_mut().find(|item| item.name == member_name) {
                        member.traits.extend(traits);
                    } else {
                        // A member supplied by a mixin is materialized locally so member-level
                        // apply statements can still affect requiredness and examples.
                        members.push(Member {
                            name: member_name.to_owned(),
                            target: String::new(),
                            traits,
                            start: usize::MAX,
                        });
                    }
                }
            }
        }

        Ok(model)
    }

    pub(crate) fn service(&self) -> Result<&Shape> {
        self.shapes
            .values()
            .find(|shape| matches!(shape.kind, ShapeKind::Service { .. }))
            .ok_or(Error::MissingService)
    }

    pub(crate) fn shape(&self, name: &str) -> Result<&Shape> {
        self.shapes
            .get(local_name(name))
            .ok_or_else(|| Error::UnknownShape(name.to_owned()))
    }

    pub(crate) fn apply_located_traits(&mut self, traits: Vec<crate::LocatedTrait>) -> Result<()> {
        for located in traits {
            let mut target = None;
            for (shape_name, shape) in &self.shapes {
                if shape.start > located.end
                    && target
                        .as_ref()
                        .is_none_or(|(_, _, start)| shape.start < *start)
                {
                    target = Some((shape_name.clone(), None, shape.start));
                }
                let members = match &shape.kind {
                    ShapeKind::Structure { members, .. } | ShapeKind::Union { members } => members,
                    ShapeKind::List { member } | ShapeKind::Map { value: member } => {
                        std::slice::from_ref(member)
                    }
                    _ => &[],
                };
                for member in members {
                    if member.start > located.end
                        && target
                            .as_ref()
                            .is_none_or(|(_, _, start)| member.start < *start)
                    {
                        target =
                            Some((shape_name.clone(), Some(member.name.clone()), member.start));
                    }
                }
            }
            let Some((shape_name, member_name, _)) = target else {
                return Err(Error::Invalid {
                    context: located.name,
                    detail: "trait is not followed by a shape or member".to_owned(),
                });
            };
            let shape = self
                .shapes
                .get_mut(&shape_name)
                .ok_or_else(|| Error::UnknownShape(shape_name.clone()))?;
            if let Some(member_name) = member_name {
                let members = match &mut shape.kind {
                    ShapeKind::Structure { members, .. } | ShapeKind::Union { members } => members,
                    ShapeKind::List { member } | ShapeKind::Map { value: member } => {
                        std::slice::from_mut(member)
                    }
                    _ => {
                        return Err(Error::Invalid {
                            context: located.name,
                            detail: format!(
                                "located member `{member_name}` belongs to a non-member shape"
                            ),
                        });
                    }
                };
                let member = members
                    .iter_mut()
                    .find(|member| member.name == member_name)
                    .ok_or_else(|| Error::Invalid {
                        context: located.name,
                        detail: format!("located member `{member_name}` is no longer present"),
                    })?;
                member.traits.insert(located.name, located.value);
            } else {
                shape.traits.insert(located.name, located.value);
            }
        }
        Ok(())
    }
}

pub(crate) fn local_name(value: &str) -> &str {
    value.rsplit('#').next().unwrap_or(value)
}

fn collect_statements<'tree>(node: Node<'tree>, statements: &mut Vec<Node<'tree>>) {
    if matches!(node.kind(), "shape_statement" | "apply_statement") {
        statements.push(node);
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_statements(child, statements);
    }
}

fn parse_shape(node: Node<'_>, source: &[u8]) -> Result<Shape> {
    let body = node
        .child_by_field_name("body")
        .ok_or_else(|| invalid("shape", "missing body"))?;
    let traits = node
        .child_by_field_name("traits")
        .map(|value| parse_traits(value, source))
        .transpose()?
        .unwrap_or_default();
    let name = child_text(body, "name", source)?.to_owned();
    let kind = match body.kind() {
        "service_statement" => parse_service(body, source)?,
        "operation_statement" => parse_operation(body, source)?,
        "structure_statement" => ShapeKind::Structure {
            members: parse_members(body, "structure_member", source)?,
            mixins: parse_mixins(body, source)?,
        },
        "union_statement" => ShapeKind::Union {
            members: parse_members(body, "union_member", source)?,
        },
        "list_statement" => ShapeKind::List {
            member: parse_collection_member(body, "list_member", "member_type", source)?,
        },
        "map_statement" => ShapeKind::Map {
            value: parse_collection_member(body, "map_value", "value_type", source)?,
        },
        "simple_shape_statement" => ShapeKind::Simple {
            smithy_type: child_text(body, "type", source)?.to_owned(),
        },
        "enum_statement" => parse_enum(body, source)?,
        other => return Err(invalid("shape", format!("unsupported shape `{other}`"))),
    };
    Ok(Shape {
        name,
        traits,
        kind,
        // Why: a shape node starts at its first trait. Use the declaration name so a
        // separately extracted semantic trait still resolves to its owning shape when
        // another ordinary Smithy trait appears before it.
        start: body
            .child_by_field_name("name")
            .map_or_else(|| body.start_byte(), |name| name.start_byte()),
    })
}

fn parse_service(node: Node<'_>, source: &[u8]) -> Result<ShapeKind> {
    let members = node
        .child_by_field_name("members")
        .ok_or_else(|| invalid("service", "missing members"))?;
    let value = parse_object(members, source)?;
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("0.0.0")
        .to_owned();
    let operations = value
        .get("operations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|name| local_name(name).to_owned())
        .collect();
    Ok(ShapeKind::Service {
        version,
        operations,
    })
}

fn parse_operation(node: Node<'_>, source: &[u8]) -> Result<ShapeKind> {
    let body = node
        .child_by_field_name("body")
        .ok_or_else(|| invalid("operation", "missing body"))?;
    let mut input = None;
    let mut output = None;
    let mut errors = Vec::new();
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        match child.kind() {
            "operation_input" => input = descendant_shape_id(child, source)?.map(str::to_owned),
            "operation_output" => output = descendant_shape_id(child, source)?.map(str::to_owned),
            "operation_errors" => {
                errors = named_children(child)
                    .into_iter()
                    .filter(|item| item.kind() == "identifier")
                    .map(|item| text(item, source).map(str::to_owned))
                    .collect::<Result<Vec<_>>>()?;
            }
            _ => {}
        }
    }
    Ok(ShapeKind::Operation {
        input,
        output,
        errors,
    })
}

fn parse_enum(node: Node<'_>, source: &[u8]) -> Result<ShapeKind> {
    let integer = child_text(node, "type", source)? == "intEnum";
    let members = node
        .child_by_field_name("members")
        .ok_or_else(|| invalid("enum", "missing members"))?;
    let mut values = Vec::new();
    let mut cursor = members.walk();
    for member in members.named_children(&mut cursor) {
        if member.kind() != "enum_member" {
            continue;
        }
        if let Some(assignment) = member.child_by_field_name("value") {
            let child = first_named_child(assignment)
                .ok_or_else(|| invalid("enum", "empty value assignment"))?;
            values.push(parse_value(child, source)?);
        } else {
            values.push(Value::String(
                child_text(member, "name", source)?.to_owned(),
            ));
        }
    }
    Ok(ShapeKind::Enum { integer, values })
}

fn parse_members(node: Node<'_>, kind: &str, source: &[u8]) -> Result<Vec<Member>> {
    let mut members = Vec::new();
    collect_nodes(node, kind, &mut |member| {
        members.push(parse_member(member, source)?);
        Ok(())
    })?;
    Ok(members)
}

fn parse_member(node: Node<'_>, source: &[u8]) -> Result<Member> {
    let name = child_text(node, "name", source)?.to_owned();
    let target = node
        .child_by_field_name("type")
        .and_then(first_shape_id)
        .map(|item| text(item, source).map(str::to_owned))
        .transpose()?
        .unwrap_or_default();
    let traits = parse_traits(node, source)?;
    Ok(Member {
        name,
        target,
        traits,
        // Why: member nodes also start at their first trait. The member name is the
        // stable boundary after every trait attached to that member.
        start: node
            .child_by_field_name("name")
            .map_or_else(|| node.start_byte(), |name| name.start_byte()),
    })
}

fn parse_collection_member(
    node: Node<'_>,
    member_kind: &str,
    type_field: &str,
    source: &[u8],
) -> Result<Member> {
    let member = find_descendant(node, member_kind)
        .ok_or_else(|| invalid("collection", format!("missing {member_kind}")))?;
    let target = member
        .child_by_field_name(type_field)
        .and_then(first_shape_id)
        .or_else(|| first_shape_id(member))
        .map(|item| text(item, source).map(str::to_owned))
        .transpose()?
        .unwrap_or_else(|| "Document".to_owned());
    Ok(Member {
        name: "member".to_owned(),
        target,
        traits: parse_traits(member, source)?,
        // Why: collection-member traits can be interleaved; the target type begins
        // after all of them and therefore identifies the correct semantic owner.
        start: member
            .child_by_field_name(type_field)
            .map_or_else(|| member.start_byte(), |target| target.start_byte()),
    })
}

fn parse_mixins(node: Node<'_>, source: &[u8]) -> Result<Vec<String>> {
    let Some(mixins) = node.child_by_field_name("mixins") else {
        return Ok(Vec::new());
    };
    let mut values = Vec::new();
    collect_nodes(mixins, "shape_id", &mut |item| {
        values.push(local_name(text(item, source)?).to_owned());
        Ok(())
    })?;
    Ok(values)
}

fn parse_application(node: Node<'_>, source: &[u8]) -> Result<(String, Traits)> {
    let target = child_text(node, "target", source)?.to_owned();
    Ok((target, parse_traits(node, source)?))
}

fn parse_traits(node: Node<'_>, source: &[u8]) -> Result<Traits> {
    let mut traits = Traits::default();
    collect_nodes(node, "trait", &mut |item| {
        if item.has_error() {
            return Ok(());
        }
        let trait_type = child_text(item, "type", source)?;
        let name = local_name(trait_type).to_owned();
        let value = named_children(item)
            .into_iter()
            .find(|child| matches!(child.kind(), "node_value" | "trait_structure"))
            .map(|child| parse_value(child, source))
            .transpose()?
            .unwrap_or(Value::Bool(true));
        traits.0.insert(name, value);
        Ok(())
    })?;
    Ok(traits)
}

fn parse_value(node: Node<'_>, source: &[u8]) -> Result<Value> {
    let node = if matches!(node.kind(), "node_value" | "value_assignment") {
        first_named_child(node).ok_or_else(|| invalid("node value", "missing value"))?
    } else {
        node
    };
    match node.kind() {
        "string_value" | "string" | "identifier" | "shape_id" => {
            let raw = text(node, source)?;
            match serde_json::from_str::<String>(raw) {
                Ok(value) => Ok(Value::String(value)),
                Err(_) => Ok(Value::String(raw.to_owned())),
            }
        }
        "number" => {
            let raw = text(node, source)?;
            if let Ok(value) = raw.parse::<i64>() {
                Ok(Value::Number(Number::from(value)))
            } else if let Ok(value) = raw.parse::<u64>() {
                Ok(Value::Number(Number::from(value)))
            } else {
                raw.parse::<f64>()
                    .ok()
                    .and_then(Number::from_f64)
                    .map(Value::Number)
                    .ok_or_else(|| invalid("number", raw))
            }
        }
        "boolean" => Ok(Value::Bool(text(node, source)? == "true")),
        "null" => Ok(Value::Null),
        "node_array" => named_children(node)
            .into_iter()
            .map(|child| parse_value(child, source))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        "node_object" | "trait_structure" => parse_object(node, source).map(Value::Object),
        other => Err(invalid(
            "node value",
            format!("unsupported value node `{other}`"),
        )),
    }
}

fn parse_object(node: Node<'_>, source: &[u8]) -> Result<Map<String, Value>> {
    let mut object = Map::new();
    let mut cursor = node.walk();
    for item in node.named_children(&mut cursor) {
        if !matches!(item.kind(), "node_object_kvp" | "trait_structure_kvp") {
            continue;
        }
        let key = child_text(item, "key", source)?;
        let key = serde_json::from_str::<String>(key).unwrap_or_else(|_| key.to_owned());
        let value = item
            .child_by_field_name("value")
            .ok_or_else(|| invalid("object", format!("missing value for `{key}`")))?;
        object.insert(key, parse_value(value, source)?);
    }
    Ok(object)
}

fn child_text<'a>(node: Node<'_>, field: &str, source: &'a [u8]) -> Result<&'a str> {
    let child = node
        .child_by_field_name(field)
        .ok_or_else(|| invalid("syntax tree", format!("missing `{field}`")))?;
    text(child, source)
}

fn text<'a>(node: Node<'_>, source: &'a [u8]) -> Result<&'a str> {
    node.utf8_text(source)
        .map_err(|error| invalid("UTF-8", error.to_string()))
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn first_named_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

fn first_shape_id(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "shape_id" {
        return Some(node);
    }
    named_children(node).into_iter().find_map(first_shape_id)
}

fn descendant_shape_id<'a>(node: Node<'_>, source: &'a [u8]) -> Result<Option<&'a str>> {
    first_shape_id(node)
        .map(|item| text(item, source))
        .transpose()
}

fn find_descendant<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    named_children(node)
        .into_iter()
        .find_map(|child| find_descendant(child, kind))
}

fn collect_nodes(
    node: Node<'_>,
    kind: &str,
    visitor: &mut impl FnMut(Node<'_>) -> Result<()>,
) -> Result<()> {
    if node.kind() == kind {
        visitor(node)?;
        return Ok(());
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_nodes(child, kind, visitor)?;
    }
    Ok(())
}

fn invalid(context: &'static str, detail: impl Into<String>) -> Error {
    Error::Invalid {
        context,
        detail: detail.into(),
    }
}
