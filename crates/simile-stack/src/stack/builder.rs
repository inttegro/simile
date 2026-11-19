use std::collections::BTreeSet;

use simile_spec::ResourceKind;

use super::supervisor::Stack;
use crate::{Clock, ErrorCode, LogicalTime, ResourceAddress, ResourceError, Result};

#[derive(Clone, Debug)]
pub struct StackBuilder {
    clock: Clock,
    resources: BTreeSet<ResourceAddress>,
}

impl StackBuilder {
    #[must_use]
    pub fn manual(start: LogicalTime) -> Self {
        Self {
            clock: Clock::manual(start),
            resources: BTreeSet::new(),
        }
    }

    #[must_use]
    pub fn realtime(start: LogicalTime) -> Self {
        Self {
            clock: Clock::realtime(start),
            resources: BTreeSet::new(),
        }
    }

    pub fn resource(mut self, kind: ResourceKind, name: impl Into<String>) -> Result<Self> {
        let address = ResourceAddress::new(kind, name)?;
        if !self.resources.insert(address) {
            return Err(ResourceError::new(
                ErrorCode::Conflict,
                "the resource is already declared",
            ));
        }
        Ok(self)
    }

    pub fn resources(
        mut self,
        resources: impl IntoIterator<Item = ResourceAddress>,
    ) -> Result<Self> {
        for resource in resources {
            if !self.resources.insert(resource) {
                return Err(ResourceError::new(
                    ErrorCode::Conflict,
                    "a resource is declared more than once",
                ));
            }
        }
        Ok(self)
    }

    pub fn build(self) -> Result<Stack> {
        Stack::new(self.clock, self.resources)
    }
}
