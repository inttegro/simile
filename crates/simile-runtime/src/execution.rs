use serde_json::Value;
use simile_spec::OutcomeProvenance;

use crate::{EffectPlan, FaultActivation, SynthesizedResponse, TransportPlan, ValidatedBody};

/// One operation invocation selected by a transport or another runtime client.
#[derive(Clone, Copy, Debug)]
pub struct OperationCall<'a> {
    pub operation: &'a str,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
}

/// One operation invocation whose request body has already been decoded by its adapter.
#[derive(Clone, Copy, Debug)]
pub struct OperationValueCall<'a> {
    pub operation: &'a str,
    pub content_type: Option<&'a str>,
    pub body: Option<&'a Value>,
}

/// An identity that makes a repeated invocation replay its first completed outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayIdentity<'a> {
    /// A caller-supplied idempotency key for a mutation.
    IdempotencyKey(&'a str),
    /// The stable identity of a resource addressed by a GET request.
    Resource(&'a str),
}

/// Optional controls for scenarios that need a specific declared response.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecutionOptions<'a> {
    pub outcome: Option<&'a str>,
    pub scenario: Option<&'a str>,
    pub fault_profile: Option<&'a str>,
    pub response_status: Option<u16>,
    pub response_content_type: Option<&'a str>,
    /// An exact state-derived response candidate. Named outcomes retain
    /// precedence when a request deliberately selects one.
    pub response_body: Option<&'a Value>,
    /// Parameters already decoded and proven by the transport adapter.
    pub parameters: Option<&'a crate::ValidatedParameters>,
    pub replay: Option<ReplayIdentity<'a>>,
    /// Reproduce response synthesis exactly when present.
    pub response_seed: Option<u64>,
}

/// The validated request and immediate response for an operation.
#[derive(Clone, Debug, PartialEq)]
pub struct OperationOutcome {
    operation: String,
    request_body: Option<ValidatedBody>,
    response: SynthesizedResponse,
    selected_outcome: Option<String>,
    provenance: Option<OutcomeProvenance>,
}

impl OperationOutcome {
    pub(crate) fn new(
        operation: String,
        request_body: Option<ValidatedBody>,
        response: SynthesizedResponse,
        selected_outcome: Option<String>,
        provenance: Option<OutcomeProvenance>,
    ) -> Self {
        Self {
            operation,
            request_body,
            response,
            selected_outcome,
            provenance,
        }
    }

    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    #[must_use]
    pub const fn request_body(&self) -> Option<&ValidatedBody> {
        self.request_body.as_ref()
    }

    #[must_use]
    pub const fn response(&self) -> &SynthesizedResponse {
        &self.response
    }

    #[must_use]
    pub fn selected_outcome(&self) -> Option<&str> {
        self.selected_outcome.as_deref()
    }

    #[must_use]
    pub const fn provenance(&self) -> Option<OutcomeProvenance> {
        self.provenance
    }

    #[must_use]
    pub fn into_parts(self) -> (String, Option<ValidatedBody>, SynthesizedResponse) {
        (self.operation, self.request_body, self.response)
    }
}

/// A completed service outcome plus the attempt-scoped delivery plan.
///
/// Replay stores only `OperationOutcome`. That boundary is what lets a
/// disconnect-after-commit affect the first attempt while an idempotent retry
/// receives the already committed result normally.
#[derive(Clone, Debug, PartialEq)]
pub struct OperationExecution {
    outcome: OperationOutcome,
    effects: EffectPlan,
    fault: Option<FaultActivation>,
    transport: TransportPlan,
}

impl OperationExecution {
    pub(crate) fn normal(outcome: OperationOutcome) -> Self {
        Self {
            outcome,
            effects: EffectPlan::default(),
            fault: None,
            transport: TransportPlan::Respond,
        }
    }

    pub(crate) fn with_fault(
        outcome: OperationOutcome,
        effects: EffectPlan,
        fault: Option<FaultActivation>,
    ) -> Self {
        let transport = fault
            .as_ref()
            .map_or(TransportPlan::Respond, FaultActivation::transport_plan);
        Self {
            outcome,
            effects,
            fault,
            transport,
        }
    }

    #[must_use]
    pub const fn outcome(&self) -> &OperationOutcome {
        &self.outcome
    }

    #[must_use]
    pub const fn fault(&self) -> Option<&FaultActivation> {
        self.fault.as_ref()
    }

    #[must_use]
    pub const fn effects(&self) -> &EffectPlan {
        &self.effects
    }

    #[must_use]
    pub const fn transport(&self) -> &TransportPlan {
        &self.transport
    }

    #[must_use]
    pub fn operation(&self) -> &str {
        self.outcome.operation()
    }

    #[must_use]
    pub const fn request_body(&self) -> Option<&ValidatedBody> {
        self.outcome.request_body()
    }

    #[must_use]
    pub const fn response(&self) -> &SynthesizedResponse {
        self.outcome.response()
    }

    #[must_use]
    pub fn selected_outcome(&self) -> Option<&str> {
        self.outcome.selected_outcome()
    }

    #[must_use]
    pub const fn provenance(&self) -> Option<OutcomeProvenance> {
        self.outcome.provenance()
    }

    #[must_use]
    pub fn into_outcome(self) -> OperationOutcome {
        self.outcome
    }
}
