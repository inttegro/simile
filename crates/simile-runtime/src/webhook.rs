use sha2::{Digest, Sha256};
use simile_artifact::Artifact;
use simile_spec::WebhookOrdering;

/// Two otherwise eligible webhook deliveries whose relative order may vary.
///
/// The scheduler sets `hard_dependency` when the later occurrence has a direct
/// or transitive `after` dependency on the earlier occurrence. Such a causal
/// edge is a contract, not a probability, and can never be reordered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebhookDeliveryPair<'a> {
    pub correlation_id: &'a str,
    pub earlier_delivery_id: &'a str,
    pub later_delivery_id: &'a str,
    pub separation_milliseconds: u64,
    pub hard_dependency: bool,
}

/// The chosen relative order for a pair of webhook deliveries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebhookOrder {
    Preserve,
    Overtake,
}

/// Why Simile preserved or reversed the ordinary delivery order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebhookOrderingReason {
    StrictPolicy,
    HardDependency,
    SerialDispatcher,
    OutsideReorderWindow,
    SeededProbability,
}

/// One reproducible ordering decision made by the webhook dispatcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebhookOrderingDecision {
    pub order: WebhookOrder,
    pub reason: WebhookOrderingReason,
    /// The deterministic sample used for a probabilistic decision, in parts
    /// per million. Structural decisions do not sample and return `None`.
    pub sample_ppm: Option<u32>,
}

/// Decide whether a later independent delivery may overtake an earlier one.
///
/// The artifact fingerprint is the service seed. Length-delimited correlation
/// and delivery identifiers make the decision stable across process restarts
/// without global mutable RNG state. This gives ordinary operation a believable
/// stochastic shape while allowing a failed test to be reproduced exactly.
#[must_use]
pub fn decide_webhook_order(
    artifact: &Artifact,
    pair: WebhookDeliveryPair<'_>,
) -> WebhookOrderingDecision {
    let policy = artifact.webhook_policy();
    if policy.ordering == WebhookOrdering::Strict {
        return preserve(WebhookOrderingReason::StrictPolicy);
    }
    if pair.hard_dependency {
        return preserve(WebhookOrderingReason::HardDependency);
    }
    if policy.concurrency < 2 {
        return preserve(WebhookOrderingReason::SerialDispatcher);
    }
    if pair.separation_milliseconds > policy.reorder_window_milliseconds {
        return preserve(WebhookOrderingReason::OutsideReorderWindow);
    }

    let mut hasher = Sha256::new();
    hasher.update(artifact.fingerprint());
    update_segment(&mut hasher, pair.correlation_id);
    update_segment(&mut hasher, pair.earlier_delivery_id);
    update_segment(&mut hasher, pair.later_delivery_id);
    let digest = hasher.finalize();
    let sample = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 1_000_000;
    WebhookOrderingDecision {
        order: if sample < policy.out_of_order_probability_ppm {
            WebhookOrder::Overtake
        } else {
            WebhookOrder::Preserve
        },
        reason: WebhookOrderingReason::SeededProbability,
        sample_ppm: Some(sample),
    }
}

const fn preserve(reason: WebhookOrderingReason) -> WebhookOrderingDecision {
    WebhookOrderingDecision {
        order: WebhookOrder::Preserve,
        reason,
        sample_ppm: None,
    }
}

fn update_segment(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use simile_compiler::{CompileRequest, Compiler, Source};
    use simile_heuristics::Policy;

    fn artifact(behavior: &str) -> Artifact {
        let service = Compiler::new()
            .compile(CompileRequest {
                openapi: Source::new(
                    "orders.yaml",
                    r#"
openapi: 3.1.0
info: { title: Orders, version: 1.0.0 }
paths:
  /orders:
    post:
      operationId: createOrder
      responses:
        "201": { description: Created }
"#,
                ),
                modules: vec![Source::new("orders.hcl", behavior)],
                heuristic_policy: Policy::Standard,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        let encoded = simile_artifact::encode(&service).unwrap_or_else(|error| panic!("{error}"));
        simile_artifact::decode(&encoded).unwrap_or_else(|error| panic!("{error}"))
    }

    fn pair() -> WebhookDeliveryPair<'static> {
        WebhookDeliveryPair {
            correlation_id: "order-42",
            earlier_delivery_id: "delivery-1",
            later_delivery_id: "delivery-2",
            separation_milliseconds: 200,
            hard_dependency: false,
        }
    }

    #[test]
    fn ordering_decisions_are_reproducible() {
        let artifact = artifact(
            r#"
webhooks {
  out_of_order_probability = 0.5
  reorder_window           = "1s"
  concurrency              = 4
}
"#,
        );
        let first = decide_webhook_order(&artifact, pair());
        let second = decide_webhook_order(&artifact, pair());
        assert_eq!(first, second);
        assert_eq!(first.reason, WebhookOrderingReason::SeededProbability);
        assert!(first.sample_ppm.is_some());
    }

    #[test]
    fn hard_dependencies_and_strict_policy_cannot_be_reordered() {
        let best_effort = artifact("webhooks { out_of_order_probability = 1 }");
        let mut dependent = pair();
        dependent.hard_dependency = true;
        assert_eq!(
            decide_webhook_order(&best_effort, dependent),
            preserve(WebhookOrderingReason::HardDependency)
        );

        let strict = artifact("webhooks { ordering = \"strict\" }");
        assert_eq!(
            decide_webhook_order(&strict, pair()),
            preserve(WebhookOrderingReason::StrictPolicy)
        );
    }

    #[test]
    fn reordering_is_bounded_by_the_configured_window() {
        let artifact = artifact(
            r#"
webhooks {
  out_of_order_probability = 1
  reorder_window           = "100ms"
}
"#,
        );
        assert_eq!(
            decide_webhook_order(&artifact, pair()),
            preserve(WebhookOrderingReason::OutsideReorderWindow)
        );
    }
}
