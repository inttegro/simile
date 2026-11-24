//! Derives server-owned response timestamp semantics from one public operation.

use std::collections::BTreeSet;

use simile_artifact::ArtifactOperation;

pub(crate) struct ResponseTimePlan {
    pub(crate) logical_time_milliseconds: i64,
    pub(crate) materialize: BTreeSet<String>,
    pub(crate) refresh: BTreeSet<String>,
}

pub(crate) fn plan(
    operation: ArtifactOperation<'_>,
    logical_time_milliseconds: Option<i64>,
) -> Option<ResponseTimePlan> {
    let logical_time_milliseconds = logical_time_milliseconds?;
    let tokens = operation_tokens(operation);
    let action = tokens
        .iter()
        .map(String::as_str)
        .find(|token| is_action(token))
        .unwrap_or_default();
    let mut materialize = BTreeSet::new();
    let mut refresh = BTreeSet::new();

    // These names describe the request-processing instant itself. Explicit request relations still
    // take precedence when a provider assigns different semantics to one of them.
    materialize.extend(
        [
            "received",
            "received_at",
            "requested",
            "requested_at",
            "processed",
            "processed_at",
            "responded",
            "responded_at",
        ]
        .into_iter()
        .map(str::to_owned),
    );

    if action == "create" {
        materialize.insert("created".to_owned());
        materialize.insert("created_at".to_owned());
        refresh.insert("updated".to_owned());
        refresh.insert("updated_at".to_owned());
    } else if is_mutation(operation.method(), action) {
        materialize.insert("updated".to_owned());
        materialize.insert("updated_at".to_owned());
    }

    for token in &tokens {
        for field in transition_fields(token) {
            materialize.insert(field);
        }
    }

    (!materialize.is_empty() || !refresh.is_empty()).then_some(ResponseTimePlan {
        logical_time_milliseconds,
        materialize,
        refresh,
    })
}

fn operation_tokens(operation: ArtifactOperation<'_>) -> Vec<String> {
    operation.operation_id().map_or_else(
        || {
            operation
                .path()
                .split('/')
                .filter(|segment| !segment.is_empty() && !segment.starts_with('{'))
                .next_back()
                .map_or_else(Vec::new, split_identifier)
        },
        split_identifier,
    )
}

fn split_identifier(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for character in value.chars() {
        if !character.is_ascii_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        if character.is_ascii_uppercase() && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        word.push(character.to_ascii_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn is_mutation(method: &str, action: &str) -> bool {
    matches!(method, "PUT" | "PATCH") || is_action(action)
}

fn is_action(token: &str) -> bool {
    matches!(
        token,
        "add"
            | "archive"
            | "cancel"
            | "close"
            | "complete"
            | "confirm"
            | "create"
            | "deactivate"
            | "delete"
            | "edit"
            | "expire"
            | "fail"
            | "finalize"
            | "fulfill"
            | "issue"
            | "open"
            | "pay"
            | "publish"
            | "refund"
            | "reject"
            | "release"
            | "remove"
            | "replace"
            | "restore"
            | "schedule"
            | "send"
            | "settle"
            | "ship"
            | "submit"
            | "succeed"
            | "update"
            | "verify"
            | "void"
    )
}

fn transition_fields(token: &str) -> Vec<String> {
    let stem = match token {
        "archive" => "archived",
        "cancel" | "canceled" | "cancelled" => "canceled",
        "close" | "closed" => "closed",
        "complete" | "completed" => "completed",
        "confirm" | "confirmed" => "confirmed",
        "deactivate" | "deactivated" => "deactivated",
        "delete" | "deleted" | "remove" | "removed" => "deleted",
        "expire" | "expired" => "expired",
        "fail" | "failed" => "failed",
        "finalize" | "finalized" => "finalized",
        "fulfill" | "fulfilled" => "fulfilled",
        "issue" | "issued" => "issued",
        "open" | "opened" => "opened",
        "pay" | "paid" => "paid",
        "publish" | "published" => "published",
        "refund" | "refunded" => "refunded",
        "reject" | "rejected" => "rejected",
        "release" | "released" => "released",
        "restore" | "restored" => "restored",
        "schedule" | "scheduled" => "scheduled",
        "send" | "sent" => "sent",
        "settle" | "settled" => "settled",
        "ship" | "shipped" => "shipped",
        "submit" | "submitted" => "submitted",
        "succeed" | "succeeded" => "succeeded",
        "verify" | "verified" => "verified",
        "void" | "voided" => "voided",
        _ => return Vec::new(),
    };
    let mut fields = vec![stem.to_owned(), format!("{stem}_at")];
    if stem == "canceled" {
        fields.push("cancelled_at".to_owned());
    }
    fields
}

#[cfg(test)]
mod tests {
    use simile_artifact::{decode, encode};
    use simile_compiler::{CompileRequest, Compiler, Source};
    use simile_heuristics::Policy;

    use super::plan;

    fn operation(operation_id: &str, method: &str, path: &str) -> simile_artifact::Artifact {
        let source = format!(
            r#"
openapi: 3.1.0
info: {{ title: Clock, version: 1.0.0 }}
paths:
  {path}:
    {method}:
      operationId: {operation_id}
      responses:
        '200': {{ description: ok }}
"#,
            method = method.to_ascii_lowercase()
        );
        let compiled = Compiler::new()
            .compile(CompileRequest {
                openapi: Source::new("clock.yml", &source),
                modules: Vec::new(),
                heuristic_policy: Policy::Standard,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        let encoded = encode(&compiled).unwrap_or_else(|error| panic!("{error}"));
        decode(&encoded).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn create_update_and_transition_operations_receive_distinct_timestamp_plans() {
        let create = operation("createPrice", "post", "/prices/create");
        let create = plan(
            create
                .operation_by_id("createPrice")
                .unwrap_or_else(|| panic!("create operation")),
            Some(1),
        )
        .unwrap_or_else(|| panic!("create clock plan"));
        assert!(create.materialize.contains("created_at"));
        assert!(create.refresh.contains("updated_at"));

        let update = operation("updatePrice", "post", "/prices/update");
        let update = plan(
            update
                .operation_by_id("updatePrice")
                .unwrap_or_else(|| panic!("update operation")),
            Some(1),
        )
        .unwrap_or_else(|| panic!("update clock plan"));
        assert!(update.materialize.contains("updated_at"));
        assert!(!update.materialize.contains("created_at"));

        let suffix_update = operation("priceUpdate", "post", "/prices/update");
        let suffix_update = plan(
            suffix_update
                .operation_by_id("priceUpdate")
                .unwrap_or_else(|| panic!("suffix update operation")),
            Some(1),
        )
        .unwrap_or_else(|| panic!("suffix update clock plan"));
        assert!(suffix_update.materialize.contains("updated_at"));

        let archive = operation("archivePrice", "post", "/prices/archive");
        let archive = plan(
            archive
                .operation_by_id("archivePrice")
                .unwrap_or_else(|| panic!("archive operation")),
            Some(1),
        )
        .unwrap_or_else(|| panic!("archive clock plan"));
        assert!(archive.materialize.contains("archived_at"));
        assert!(archive.materialize.contains("updated_at"));
    }
}
