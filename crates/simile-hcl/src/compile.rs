use simile_spec::{ApiContract, AsyncOperation, Behavior, BehaviorSpec, Resource};

use crate::{Error, Module, Result};

pub fn compile(
    modules: impl IntoIterator<Item = Module>,
    api: &ApiContract,
) -> Result<BehaviorSpec> {
    let mut spec = BehaviorSpec::default();

    for module in modules {
        collect_resources(&mut spec, module.resources)?;
        collect_async_operations(&mut spec, module.async_operations)?;
        collect_behaviors(&mut spec, module.behaviors)?;
    }

    validate_resources(&spec)?;
    validate_behaviors(&spec, api)?;
    Ok(spec)
}

fn collect_resources(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::ResourceDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let reference = declaration.reference();
        let resource = Resource {
            kind: declaration.kind,
            name: declaration.name,
        };
        if spec.resources.insert(reference.clone(), resource).is_some() {
            return Err(Error::DuplicateResource {
                resource: reference.to_string(),
            });
        }
    }
    Ok(())
}

fn collect_async_operations(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::AsyncDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let name = declaration.name;
        let operation = AsyncOperation {
            name: name.clone(),
            uses: declaration.uses,
        };
        if spec
            .async_operations
            .insert(name.clone(), operation)
            .is_some()
        {
            return Err(Error::DuplicateAsyncOperation { name });
        }
    }
    Ok(())
}

fn collect_behaviors(
    spec: &mut BehaviorSpec,
    declarations: impl IntoIterator<Item = crate::BehaviorDeclaration>,
) -> Result<()> {
    for declaration in declarations {
        let operation_id = declaration.operation_id;
        let behavior = Behavior {
            operation_id: operation_id.clone(),
            response_status: declaration.response_status,
            triggers: declaration.triggers,
        };
        if spec
            .behaviors
            .insert(operation_id.clone(), behavior)
            .is_some()
        {
            return Err(Error::DuplicateBehavior { operation_id });
        }
    }
    Ok(())
}

fn validate_resources(spec: &BehaviorSpec) -> Result<()> {
    for operation in spec.async_operations.values() {
        for resource in &operation.uses {
            if !spec.resources.contains_key(resource) {
                return Err(Error::UnknownResource {
                    operation: operation.name.clone(),
                    resource: resource.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn validate_behaviors(spec: &BehaviorSpec, api: &ApiContract) -> Result<()> {
    for behavior in spec.behaviors.values() {
        let operation =
            api.operation_by_id(&behavior.operation_id)
                .ok_or_else(|| Error::UnknownOperation {
                    operation_id: behavior.operation_id.clone(),
                })?;
        if let Some(status) = behavior.response_status {
            if !operation.declares_status(status) {
                return Err(Error::UndeclaredResponse {
                    operation_id: behavior.operation_id.clone(),
                    status,
                });
            }
        }
        for target in &behavior.triggers {
            if !spec.async_operations.contains_key(target) {
                return Err(Error::UnknownAsyncTarget {
                    operation_id: behavior.operation_id.clone(),
                    target: target.clone(),
                });
            }
        }
    }
    Ok(())
}
