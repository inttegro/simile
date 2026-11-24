export interface RunResult {
  exitCode: number | null;
  stdout: string;
  stderr: string;
}

export interface VerificationVerdict {
  conclusive: boolean;
  reproduced: boolean;
  fingerprint: string | null;
  reason: string | null;
  reasonCode: string | null;
}

interface PreparedReport {
  report?: {
    verification?: {
      reproduced?: boolean;
      fingerprint?: string;
    };
  };
}

/**
 * Classify a verifier process result without confusing incompatibility with disproof.
 *
 * Simile reports an actual deterministic mismatch explicitly. Every other non-zero result may be
 * a verifier capability gap (including a future bundle or artifact format), so the receiver must
 * preserve it for a later verifier instead of rejecting the reporter's evidence.
 */
export function classifyRunResult(output: RunResult): VerificationVerdict {
  if (output.exitCode !== 0) {
    const diagnostic = output.stderr || output.stdout;
    const reason = conciseDiagnostic(diagnostic);
    const mismatch = diagnostic.includes("did not reproduce its recorded behavior");
    return {
      conclusive: mismatch,
      reproduced: false,
      fingerprint: null,
      reason,
      reasonCode: mismatch ? "not_reproduced" : "verifier_could_not_prove",
    };
  }

  try {
    const prepared = JSON.parse(output.stdout) as PreparedReport;
    const verification = prepared.report?.verification;
    if (typeof verification?.reproduced !== "boolean") {
      return inconclusive("verifier output did not contain a reproduction result");
    }
    if (verification.reproduced && !verification.fingerprint) {
      return inconclusive("verifier output did not contain a proof fingerprint");
    }
    return {
      conclusive: true,
      reproduced: verification.reproduced,
      fingerprint: verification.fingerprint || null,
      reason: verification.reproduced ? null : "reproduction did not match",
      reasonCode: verification.reproduced ? null : "not_reproduced",
    };
  } catch {
    return inconclusive("verifier output was not valid JSON");
  }
}

function inconclusive(reason: string): VerificationVerdict {
  return {
    conclusive: false,
    reproduced: false,
    fingerprint: null,
    reason,
    reasonCode: "verifier_could_not_prove",
  };
}

function conciseDiagnostic(value: unknown): string {
  const last = String(value || "verification could not run")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .at(-1);
  return String(last || "verification could not run").slice(0, 2000);
}
