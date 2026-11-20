//! Parses the OpenAPI extension and resolves every selector against compiled schemas.

use serde_json::Value;

use super::{Expression, ParameterSource, Plan, Source, parse};
use crate::soundness::path;
use crate::{CompileError, RelationParameter, Result, SchemaCatalog, SchemaRef};

impl Plan {
    pub(crate) fn compile(
        definition: &Value,
        request_schemas: Vec<SchemaRef>,
        parameters: &[RelationParameter<'_>],
        response_schema: SchemaRef,
        catalog: &SchemaCatalog,
        field: &str,
    ) -> Result<Self> {
        let (mappings, derivations, rules) = parse::definition(definition, field)?;
        let mut plan = Self {
            request_schemas,
            parameter_schemas: parameters
                .iter()
                .filter_map(|parameter| {
                    parameter.schema.map(|schema| {
                        (
                            ParameterSource {
                                location: parameter.location,
                                name: normalize_parameter_name(parameter.location, parameter.name),
                            },
                            schema.schema,
                        )
                    })
                })
                .collect(),
            mappings,
            derivations,
            rules,
        };
        plan.resolve(catalog, response_schema, field)?;
        Ok(plan)
    }

    pub(crate) fn is_well_formed(
        &self,
        catalog: &SchemaCatalog,
        response_schema: SchemaRef,
    ) -> bool {
        let mut plan = self.clone();
        plan.resolve(catalog, response_schema, "x-simile-from-request")
            .is_ok()
    }

    fn resolve(
        &mut self,
        catalog: &SchemaCatalog,
        response_schema: SchemaRef,
        field: &str,
    ) -> Result<()> {
        if self
            .request_schemas
            .iter()
            .any(|schema| !catalog.contains(*schema))
            || self
                .parameter_schemas
                .values()
                .any(|schema| !catalog.contains(*schema))
            || !catalog.contains(response_schema)
        {
            return Err(invalid(field, "relation references an unknown schema"));
        }
        let request_schemas = self.request_schemas.clone();
        let parameter_schemas = self.parameter_schemas.clone();
        for (index, mapping) in self.mappings.iter_mut().enumerate() {
            let item = format!("{field}.bindings.{index}");
            mapping
                .source
                .resolve(&request_schemas, &parameter_schemas, catalog)
                .ok_or_else(|| {
                    invalid(&item, "source does not resolve in the operation request")
                })?;
            mapping.target_schema =
                path::resolve_property(catalog, response_schema.0, &mapping.target)
                    .ok_or_else(|| {
                        invalid(&item, "target does not resolve in the response schema")
                    })?
                    .schema;
        }
        for (index, derivation) in self.derivations.iter().enumerate() {
            let item = format!("{field}.bindings.{index}");
            path::resolve_property(catalog, response_schema.0, &derivation.target)
                .ok_or_else(|| invalid(&item, "target does not resolve in the response schema"))?;
            derivation.value.check(self, catalog, &item)?;
        }
        for (index, rule) in self.rules.iter().enumerate() {
            let item = format!("{field}.rules.{index}");
            for condition in &rule.when {
                condition
                    .source
                    .resolve(&request_schemas, &parameter_schemas, catalog)
                    .ok_or_else(|| {
                        invalid(&item, "condition does not resolve in the operation request")
                    })?;
            }
            for (target, value) in &rule.set {
                let target = path::resolve_property(catalog, response_schema.0, target)
                    .ok_or_else(|| invalid(&item, "set target is not a response property"))?;
                if !catalog.validate(SchemaRef(target.schema), value).is_empty() {
                    return Err(invalid(
                        &item,
                        "set value is rejected by the response schema",
                    ));
                }
            }
            for target in &rule.omit {
                let target = path::resolve_property(catalog, response_schema.0, target)
                    .ok_or_else(|| invalid(&item, "omit target is not a response property"))?;
                if target.required {
                    return Err(invalid(
                        &item,
                        "a required response property cannot be omitted",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Expression {
    fn check(&self, plan: &Plan, catalog: &SchemaCatalog, field: &str) -> Result<()> {
        match self {
            Self::Request(source) => {
                source
                    .resolve(&plan.request_schemas, &plan.parameter_schemas, catalog)
                    .ok_or_else(|| invalid(field, "request path does not resolve"))?;
            }
            Self::Relative(path) => {
                path::resolve_selector_syntax(path).map_err(|reason| invalid(field, &reason))?;
            }
            Self::Literal(_) => {}
            Self::Add(values) | Self::Multiply(values) => {
                if values.len() < 2 {
                    return Err(invalid(
                        field,
                        "numeric operation requires at least two values",
                    ));
                }
                for value in values {
                    value.check(plan, catalog, field)?;
                }
            }
            Self::Concat(values) | Self::Coalesce(values) => {
                if values.len() < 2 {
                    return Err(invalid(field, "operation requires at least two values"));
                }
                for value in values {
                    value.check(plan, catalog, field)?;
                }
            }
            Self::Substring { value, .. } | Self::Mask { value, .. } => {
                value.check(plan, catalog, field)?;
            }
            Self::Sum { each, value } => {
                let selected = plan
                    .request_schemas
                    .iter()
                    .find_map(|request| path::resolve_selector(catalog, request.0, each))
                    .ok_or_else(|| invalid(field, "sum selector does not resolve"))?;
                if !selected.multiple {
                    return Err(invalid(
                        field,
                        "sum selector must contain an array wildcard",
                    ));
                }
                value.check_relative(field)?;
            }
        }
        Ok(())
    }

    fn check_relative(&self, field: &str) -> Result<()> {
        match self {
            Self::Relative(path) => {
                path::resolve_selector_syntax(path).map_err(|reason| invalid(field, &reason))
            }
            Self::Literal(_) => Ok(()),
            Self::Add(values) | Self::Multiply(values) => {
                for value in values {
                    value.check_relative(field)?;
                }
                Ok(())
            }
            Self::Concat(values) | Self::Coalesce(values) => {
                for value in values {
                    value.check_relative(field)?;
                }
                Ok(())
            }
            Self::Substring { value, .. } | Self::Mask { value, .. } => value.check_relative(field),
            Self::Request(_) | Self::Sum { .. } => Err(invalid(
                field,
                "sum values may use only relative paths, literals, add, and multiply",
            )),
        }
    }
}

impl Source {
    fn resolve(
        &self,
        request_schemas: &[SchemaRef],
        parameter_schemas: &std::collections::BTreeMap<ParameterSource, SchemaRef>,
        catalog: &SchemaCatalog,
    ) -> Option<SchemaRef> {
        match self {
            Self::Body(selector) => request_schemas.iter().find_map(|root| {
                path::resolve_selector(catalog, root.0, selector)
                    .map(|selected| SchemaRef(selected.schema))
            }),
            Self::Parameter(source) => parameter_schemas.get(source).copied(),
        }
    }
}

fn normalize_parameter_name(location: simile_spec::ParameterLocation, name: &str) -> String {
    if location == simile_spec::ParameterLocation::Header {
        name.to_ascii_lowercase()
    } else {
        name.to_owned()
    }
}

fn invalid(field: &str, reason: &str) -> CompileError {
    CompileError::InvalidConstraint {
        field: field.to_owned(),
        reason: reason.to_owned(),
    }
}
