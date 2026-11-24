export type VerificationDecision =
  | { status: "accepted"; fingerprint: string; reason: null; reasonCode: null }
  | { status: "rejected"; fingerprint: string | null; reason: string; reasonCode: string | null }
  | {
      status: "pending_verification";
      fingerprint: string | null;
      reason: string;
      reasonCode: string | null;
    };

/**
 * Turn an untrusted verifier response into a report-state decision.
 *
 * A verifier can disprove evidence only when it says its run was conclusive. This distinction is
 * what prevents an older verifier from rejecting evidence merely because a newer format could not
 * be decoded yet.
 */
export function decideVerification(
  value: unknown,
  expectedFingerprint: string,
): VerificationDecision {
  const result = objectValue(value);
  const fingerprint = typeof result.fingerprint === "string" ? result.fingerprint : null;
  const reason = textValue(result.reason, "the verifier could not yet prove this evidence");
  const reasonCode = typeof result.reasonCode === "string" ? result.reasonCode : null;

  if (result.conclusive !== true || typeof result.reproduced !== "boolean") {
    return {
      status: "pending_verification",
      fingerprint,
      reason,
      reasonCode,
    };
  }
  if (result.reproduced === true && fingerprint === expectedFingerprint) {
    return { status: "accepted", fingerprint, reason: null, reasonCode: null };
  }
  return {
    status: "rejected",
    fingerprint,
    reason:
      result.reproduced === true && fingerprint !== expectedFingerprint
        ? "verification fingerprint did not match"
        : textValue(result.reason, "reproduction did not match"),
    reasonCode,
  };
}

function objectValue(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("verifier response must be an object");
  }
  return value as Record<string, unknown>;
}

function textValue(value: unknown, fallback: string): string {
  return (typeof value === "string" && value.trim() ? value : fallback).slice(0, 2000);
}
