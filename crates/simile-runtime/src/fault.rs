use std::collections::HashMap;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use simile_artifact::{Artifact, ArtifactFault, ArtifactFaultProfile};
use simile_spec::{FaultAction, FaultPhase};

use crate::{Error, Result};

/// The runtime-level transport instruction produced by a fault activation.
///
/// This is deliberately protocol-neutral. The HTTP adapter decides how a
/// disconnect is represented on the wire; another adapter can honor the same
/// instruction without importing Axum or HTTP concepts into the runtime.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum TransportPlan {
    #[default]
    Respond,
    RespondAfter {
        milliseconds: u64,
    },
    DisconnectAfterCommit,
}

/// Why an environmental fault was activated for this attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaultSource {
    Scenario(String),
    Profile(String),
}

/// Auditable evidence of the environmental fault selected for an attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaultActivation {
    name: String,
    phase: FaultPhase,
    action: FaultAction,
    source: FaultSource,
}

#[derive(Debug)]
pub(crate) struct FaultController {
    profiles: HashMap<String, ProfileState>,
}

#[derive(Debug)]
struct ProfileState {
    definition: ArtifactFaultProfile,
    seed: u64,
    rng: StdRng,
    consecutive: u32,
    cooldown_remaining: u32,
}

impl FaultController {
    pub(crate) fn new(artifact: &Artifact) -> Self {
        let profiles = artifact
            .fault_profiles()
            .map(|definition| {
                let seed = definition.seed().unwrap_or_else(rand::random);
                let name = definition.name().to_owned();
                (
                    name,
                    ProfileState {
                        definition,
                        seed,
                        rng: StdRng::seed_from_u64(seed),
                        consecutive: 0,
                        cooldown_remaining: 0,
                    },
                )
            })
            .collect();
        Self { profiles }
    }

    pub(crate) fn seed(&self, profile: &str) -> Option<u64> {
        self.profiles.get(profile).map(|state| state.seed)
    }

    pub(crate) fn activate(
        &mut self,
        artifact: &Artifact,
        operation: &str,
        scenario: Option<&str>,
        profile: Option<&str>,
    ) -> Result<Option<FaultActivation>> {
        if let Some(scenario) = scenario {
            if let Some(name) = artifact.scenario_fault(scenario, operation) {
                let fault = artifact.fault(name).ok_or_else(|| Error::UnknownFault {
                    fault: name.to_owned(),
                })?;
                return Ok(Some(activation(
                    fault,
                    FaultSource::Scenario(scenario.to_owned()),
                )));
            }
        }
        let Some(profile_name) = profile else {
            return Ok(None);
        };
        let state =
            self.profiles
                .get_mut(profile_name)
                .ok_or_else(|| Error::UnknownFaultProfile {
                    profile: profile_name.to_owned(),
                })?;
        if !state.definition.operations().is_empty()
            && !state
                .definition
                .operations()
                .iter()
                .any(|candidate| candidate == operation)
        {
            return Ok(None);
        }
        if state.cooldown_remaining > 0 {
            state.cooldown_remaining -= 1;
            state.consecutive = 0;
            return Ok(None);
        }
        let selected = state.rng.random_range(0..1_000_000) < state.definition.probability_ppm();
        if !selected {
            state.consecutive = 0;
            return Ok(None);
        }
        let index = state.rng.random_range(0..state.definition.faults().len());
        let name = &state.definition.faults()[index];
        let fault = artifact.fault(name).ok_or_else(|| Error::UnknownFault {
            fault: name.clone(),
        })?;
        state.consecutive += 1;
        if state.consecutive >= state.definition.max_consecutive() {
            state.cooldown_remaining = state.definition.cooldown_requests().max(1);
        }
        Ok(Some(activation(
            fault,
            FaultSource::Profile(profile_name.to_owned()),
        )))
    }
}

fn activation(fault: ArtifactFault, source: FaultSource) -> FaultActivation {
    FaultActivation {
        name: fault.name().to_owned(),
        phase: fault.phase(),
        action: fault.action().clone(),
        source,
    }
}

impl FaultActivation {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn phase(&self) -> FaultPhase {
        self.phase
    }

    #[must_use]
    pub const fn action(&self) -> &FaultAction {
        &self.action
    }

    #[must_use]
    pub const fn source(&self) -> &FaultSource {
        &self.source
    }

    #[must_use]
    pub const fn transport_plan(&self) -> TransportPlan {
        match self.action {
            FaultAction::Delay { milliseconds } => TransportPlan::RespondAfter { milliseconds },
            FaultAction::Disconnect => TransportPlan::DisconnectAfterCommit,
        }
    }
}
