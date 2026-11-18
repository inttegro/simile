//! Parses the OpenAPI extension and resolves every selector against compiled schemas.

use serde_json::Value;

use super::{Expression, Plan, parse};
use crate::soundness::path;
use crate::{CompileError, Result, SchemaCatalog, SchemaRef};

impl Plan {
    pub(crate) fn compile(
        definition: &Value,
        request_schema: SchemaRef,
        response_schema: SchemaRef,
        catalog: &SchemaCatalog,
        field: &str,
    ) -> Result<Self> {
        let (mappings, derivations, rules) = parse::definition(definition, field)?;
        let mut plan = Self {
            request_schema,
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
        if !catalog.contains(self.request_schema) || !catalog.contains(response_schema) {
            return Err(invalid(field, "relation references an unknown schema"));
        }
        for (index, mapping) in self.mappings.iter_mut().enumerate() {
            let item = format!("{field}.bindings.{index}");
            path::resolve_selector(catalog, self.request_schema.0, &mapping.source)
                .ok_or_else(|| invalid(&item, "source does not resolve in the request schema"))?;
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
            derivation
                .value
                .check(catalog, self.request_schema, &item)?;
        }
        for (index, rule) in self.rules.iter().enumerate() {
            let item = format!("{field}.rules.{index}");
            for condition in &rule.when {
                path::resolve_selector(catalog, self.request_schema.0, &condition.path)
                    .ok_or_else(|| invalid(&item, "condition does not resolve in the request"))?;
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
    fn check(&self, catalog: &SchemaCatalog, request: SchemaRef, field: &str) -> Result<()> {
        match self {
            Self::Request(path) => {
                path::resolve_selector(catalog, request.0, path)
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
                    value.check(catalog, request, field)?;
                }
            }
            Self::Sum { each, value } => {
                let selected = path::resolve_selector(catalog, request.0, each)
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
            Self::Request(_) | Self::Sum { .. } => Err(invalid(
                field,
                "sum values may use only relative paths, literals, add, and multiply",
            )),
        }
    }
}

fn invalid(field: &str, reason: &str) -> CompileError {
    CompileError::InvalidConstraint {
        field: field.to_owned(),
        reason: reason.to_owned(),
    }
}
