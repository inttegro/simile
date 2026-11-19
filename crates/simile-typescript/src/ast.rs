use std::collections::BTreeMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Declaration, Statement, TSInterfaceDeclaration, TSLiteral, TSModuleDeclaration,
    TSModuleDeclarationBody, TSModuleDeclarationName, TSSignature, TSType, TSTypeAliasDeclaration,
    TSTypeName,
};
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde_json::{Number, Value};

use crate::Source;
use crate::error::{Error, Result};

#[derive(Clone, Debug)]
pub(crate) struct Property {
    pub(crate) name: String,
    pub(crate) optional: bool,
    pub(crate) value: TypeExpr,
}

#[derive(Clone, Debug)]
pub(crate) enum TypeExpr {
    Any,
    Never,
    Null,
    Boolean,
    Number,
    String,
    Literal(Value),
    Template(Vec<String>),
    Array(Box<TypeExpr>),
    Tuple(Vec<TypeExpr>),
    Object {
        properties: Vec<Property>,
        additional: Option<Box<TypeExpr>>,
    },
    Reference {
        name: String,
        arguments: Vec<TypeExpr>,
    },
    Union(Vec<TypeExpr>),
    Intersection(Vec<TypeExpr>),
}

impl TypeExpr {
    pub(crate) fn property(&self, name: &str) -> Option<&TypeExpr> {
        match self {
            Self::Object { properties, .. } => properties
                .iter()
                .find(|property| property.name == name)
                .map(|property| &property.value),
            _ => None,
        }
    }

    pub(crate) fn properties(&self) -> Option<&[Property]> {
        match self {
            Self::Object { properties, .. } => Some(properties),
            _ => None,
        }
    }

    pub(crate) fn literal_string(&self) -> Option<&str> {
        match self {
            Self::Literal(Value::String(value)) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn literal_bool(&self) -> Option<bool> {
        match self {
            Self::Literal(Value::Bool(value)) => Some(*value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ContractAst {
    pub(crate) declarations: BTreeMap<String, TypeExpr>,
    pub(crate) unsupported: BTreeMap<String, String>,
}

impl ContractAst {
    pub(crate) fn parse(sources: &[Source<'_>]) -> Result<Self> {
        let mut contract = Self::default();
        for source in sources {
            parse_source(source, &mut contract)?;
        }
        Ok(contract)
    }
}

pub(crate) fn module_specifiers(source: &Source<'_>) -> Result<Vec<String>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source.contents, SourceType::d_ts()).parse();
    if !parsed.errors.is_empty() {
        return Err(Error::Parse {
            name: source.name.to_owned(),
            detail: parsed
                .errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        });
    }
    let mut specifiers = Vec::new();
    for statement in &parsed.program.body {
        let value = match statement {
            Statement::ImportDeclaration(declaration) => Some(declaration.source.value.as_str()),
            Statement::ExportNamedDeclaration(declaration) => declaration
                .source
                .as_ref()
                .map(|source| source.value.as_str()),
            Statement::ExportAllDeclaration(declaration) => Some(declaration.source.value.as_str()),
            _ => None,
        };
        if let Some(value) = value {
            specifiers.push(value.to_owned());
        }
    }
    specifiers.sort();
    specifiers.dedup();
    Ok(specifiers)
}

fn parse_source(source: &Source<'_>, contract: &mut ContractAst) -> Result<()> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source.contents, SourceType::d_ts()).parse();
    if !parsed.errors.is_empty() {
        return Err(Error::Parse {
            name: source.name.to_owned(),
            detail: parsed
                .errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        });
    }
    collect_statements(
        parsed.program.body.as_slice(),
        None,
        source.name,
        source.contents,
        contract,
    )
}

fn collect_statements(
    statements: &[Statement<'_>],
    namespace: Option<&str>,
    source_name: &str,
    source: &str,
    contract: &mut ContractAst,
) -> Result<()> {
    for statement in statements {
        match statement {
            Statement::TSTypeAliasDeclaration(declaration) => {
                collect_alias(declaration, namespace, source_name, contract)?;
            }
            Statement::TSInterfaceDeclaration(declaration) => {
                collect_interface(declaration, namespace, source_name, source, contract)?;
            }
            Statement::TSModuleDeclaration(declaration) => {
                collect_module(declaration, namespace, source_name, source, contract)?;
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(declaration) = &export.declaration {
                    collect_declaration(declaration, namespace, source_name, source, contract)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_declaration(
    declaration: &Declaration<'_>,
    namespace: Option<&str>,
    source_name: &str,
    source: &str,
    contract: &mut ContractAst,
) -> Result<()> {
    match declaration {
        Declaration::TSTypeAliasDeclaration(declaration) => {
            collect_alias(declaration, namespace, source_name, contract)
        }
        Declaration::TSInterfaceDeclaration(declaration) => {
            collect_interface(declaration, namespace, source_name, source, contract)
        }
        Declaration::TSModuleDeclaration(declaration) => {
            collect_module(declaration, namespace, source_name, source, contract)
        }
        _ => Ok(()),
    }
}

fn collect_alias(
    declaration: &TSTypeAliasDeclaration<'_>,
    namespace: Option<&str>,
    source_name: &str,
    contract: &mut ContractAst,
) -> Result<()> {
    if declaration.type_parameters.is_some() {
        return Ok(());
    }
    let name = qualify(namespace, declaration.id.name.as_str());
    match convert_type(&declaration.type_annotation, source_name) {
        Ok(value) => {
            contract.declarations.insert(name, value);
        }
        Err(Error::Unsupported { detail, .. }) => {
            contract.unsupported.insert(name, detail);
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

fn collect_interface(
    declaration: &TSInterfaceDeclaration<'_>,
    namespace: Option<&str>,
    source_name: &str,
    source: &str,
    contract: &mut ContractAst,
) -> Result<()> {
    if declaration.type_parameters.is_some() {
        return Ok(());
    }
    let name = qualify(namespace, declaration.id.name.as_str());
    let object = match convert_signatures(declaration.body.body.as_slice(), source_name) {
        Ok(object) => object,
        Err(Error::Unsupported { detail, .. }) => {
            contract.unsupported.insert(name, detail);
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let mut values = Vec::new();
    for heritage in &declaration.extends {
        let start = usize::try_from(heritage.span.start).unwrap_or(0);
        let end = usize::try_from(heritage.span.end).unwrap_or(start);
        let inherited = source.get(start..end).unwrap_or_default().trim();
        if !inherited.is_empty() {
            values.push(TypeExpr::Reference {
                name: inherited.to_owned(),
                arguments: Vec::new(),
            });
        }
    }
    values.push(object);
    let value = if values.len() == 1 {
        values.pop().unwrap_or(TypeExpr::Any)
    } else {
        TypeExpr::Intersection(values)
    };
    contract.declarations.insert(name, value);
    Ok(())
}

fn collect_module(
    declaration: &TSModuleDeclaration<'_>,
    namespace: Option<&str>,
    source_name: &str,
    source: &str,
    contract: &mut ContractAst,
) -> Result<()> {
    let local = match &declaration.id {
        TSModuleDeclarationName::Identifier(identifier) => identifier.name.as_str(),
        TSModuleDeclarationName::StringLiteral(value) => value.value.as_str(),
    };
    let qualified = qualify(namespace, local);
    match &declaration.body {
        Some(TSModuleDeclarationBody::TSModuleBlock(block)) => collect_statements(
            block.body.as_slice(),
            Some(&qualified),
            source_name,
            source,
            contract,
        ),
        Some(TSModuleDeclarationBody::TSModuleDeclaration(nested)) => {
            collect_module(nested, Some(&qualified), source_name, source, contract)
        }
        None => Ok(()),
    }
}

fn convert_type(value: &TSType<'_>, source_name: &str) -> Result<TypeExpr> {
    Ok(match value {
        TSType::TSAnyKeyword(_) | TSType::TSUnknownKeyword(_) | TSType::TSObjectKeyword(_) => {
            TypeExpr::Any
        }
        TSType::TSNeverKeyword(_) | TSType::TSUndefinedKeyword(_) | TSType::TSVoidKeyword(_) => {
            TypeExpr::Never
        }
        TSType::TSNullKeyword(_) => TypeExpr::Null,
        TSType::TSBooleanKeyword(_) => TypeExpr::Boolean,
        TSType::TSNumberKeyword(_) | TSType::TSBigIntKeyword(_) => TypeExpr::Number,
        TSType::TSStringKeyword(_) => TypeExpr::String,
        TSType::TSArrayType(array) => {
            TypeExpr::Array(Box::new(convert_type(&array.element_type, source_name)?))
        }
        TSType::TSTupleType(tuple) => TypeExpr::Tuple(
            tuple
                .element_types
                .iter()
                .map(|element| convert_type(element.to_ts_type(), source_name))
                .collect::<Result<Vec<_>>>()?,
        ),
        TSType::TSTypeLiteral(object) => {
            convert_signatures(object.members.as_slice(), source_name)?
        }
        TSType::TSLiteralType(literal) => TypeExpr::Literal(convert_literal(&literal.literal)?),
        TSType::TSTemplateLiteralType(template) => TypeExpr::Template(
            template
                .quasis
                .iter()
                .map(|quasi| {
                    quasi
                        .value
                        .cooked
                        .as_ref()
                        .unwrap_or(&quasi.value.raw)
                        .as_str()
                        .to_owned()
                })
                .collect(),
        ),
        TSType::TSTypeReference(reference) => TypeExpr::Reference {
            name: type_name(&reference.type_name),
            arguments: reference
                .type_arguments
                .as_ref()
                .map(|arguments| {
                    arguments
                        .params
                        .iter()
                        .map(|argument| convert_type(argument, source_name))
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default(),
        },
        TSType::TSUnionType(union) => TypeExpr::Union(
            union
                .types
                .iter()
                .map(|item| convert_type(item, source_name))
                .collect::<Result<Vec<_>>>()?,
        ),
        TSType::TSIntersectionType(intersection) => TypeExpr::Intersection(
            intersection
                .types
                .iter()
                .map(|item| convert_type(item, source_name))
                .collect::<Result<Vec<_>>>()?,
        ),
        TSType::TSParenthesizedType(parenthesized) => {
            convert_type(&parenthesized.type_annotation, source_name)?
        }
        TSType::TSTypeOperatorType(operator) => {
            convert_type(&operator.type_annotation, source_name)?
        }
        other => {
            return Err(Error::Unsupported {
                name: source_name.to_owned(),
                detail: type_kind(other).to_string(),
            });
        }
    })
}

fn convert_signatures(signatures: &[TSSignature<'_>], source_name: &str) -> Result<TypeExpr> {
    let mut properties = Vec::new();
    let mut additional = None;
    for signature in signatures {
        match signature {
            TSSignature::TSPropertySignature(property) => {
                let name = property
                    .key
                    .static_name()
                    .map(|name| name.into_owned())
                    .ok_or_else(|| Error::Unsupported {
                        name: source_name.to_owned(),
                        detail: "computed property name".to_owned(),
                    })?;
                let value = property
                    .type_annotation
                    .as_ref()
                    .map(|annotation| convert_type(&annotation.type_annotation, source_name))
                    .transpose()?
                    .unwrap_or(TypeExpr::Any);
                properties.push(Property {
                    name,
                    optional: property.optional,
                    value,
                });
            }
            TSSignature::TSIndexSignature(index) => {
                additional = Some(Box::new(convert_type(
                    &index.type_annotation.type_annotation,
                    source_name,
                )?));
            }
            _ => {
                return Err(Error::Unsupported {
                    name: source_name.to_owned(),
                    detail: "method and call signatures are not payload fields".to_owned(),
                });
            }
        }
    }
    Ok(TypeExpr::Object {
        properties,
        additional,
    })
}

fn convert_literal(value: &TSLiteral<'_>) -> Result<Value> {
    match value {
        TSLiteral::BooleanLiteral(value) => Ok(Value::Bool(value.value)),
        TSLiteral::NumericLiteral(value) => Number::from_f64(value.value)
            .map(Value::Number)
            .ok_or_else(|| Error::Unsupported {
                name: "literal".to_owned(),
                detail: "non-finite number".to_owned(),
            }),
        TSLiteral::StringLiteral(value) => Ok(Value::String(value.value.as_str().to_owned())),
        _ => Err(Error::Unsupported {
            name: "literal".to_owned(),
            detail: "bigint, unary, and expression template literals".to_owned(),
        }),
    }
}

fn type_name(value: &TSTypeName<'_>) -> String {
    match value {
        TSTypeName::IdentifierReference(identifier) => identifier.name.as_str().to_owned(),
        TSTypeName::QualifiedName(qualified) => {
            format!("{}.{}", type_name(&qualified.left), qualified.right.name)
        }
    }
}

fn qualify(namespace: Option<&str>, name: &str) -> String {
    namespace.map_or_else(
        || name.to_owned(),
        |namespace| format!("{namespace}.{name}"),
    )
}

fn type_kind(value: &TSType<'_>) -> &'static str {
    match value {
        TSType::TSConditionalType(_) => "conditional type",
        TSType::TSMappedType(_) => "mapped type",
        TSType::TSIndexedAccessType(_) => "indexed access type",
        TSType::TSImportType(_) => "import type",
        TSType::TSFunctionType(_) | TSType::TSConstructorType(_) => "function type",
        _ => "unsupported declaration type",
    }
}
