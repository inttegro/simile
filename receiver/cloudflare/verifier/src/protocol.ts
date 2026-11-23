const REPORT_ID = /^rpt_[a-z0-9_]{1,60}$/;
const SERVICE = /^[a-z][a-z0-9_-]{0,63}$/;

export const REQUIRED_EVIDENCE = [
  "bundle.json",
  "report.json",
  "request.json",
  "service.similec",
];
export const OPTIONAL_EVIDENCE = ["request.body"];

export interface VerificationRequest {
  reportId: string;
  objectPrefix: string;
}

export function validateVerificationRequest(
  value: unknown,
  allowedServices: ReadonlySet<string>,
): VerificationRequest {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new VerificationError("verification request must be an object");
  }
  const input = value as Record<string, unknown>;
  const reportId = requiredString(input.reportId, "reportId");
  if (!REPORT_ID.test(reportId)) throw new VerificationError("reportId is invalid");
  const objectPrefix = requiredString(input.objectPrefix, "objectPrefix");
  const expectedPrefix = [...allowedServices]
    .map((service) => `quarantine/${service}/${reportId}`)
    .find((prefix) => prefix === objectPrefix);
  if (!expectedPrefix) throw new VerificationError("objectPrefix is outside the report quarantine");
  return { reportId, objectPrefix };
}

export function parseAllowedServices(value: unknown): Set<string> {
  const services = new Set(
    String(value || "")
      .split(",")
      .map((service) => service.trim())
      .filter(Boolean),
  );
  if (services.size === 0 || [...services].some((service) => !SERVICE.test(service))) {
    throw new Error("ALLOWED_SERVICES must contain lowercase public service identifiers");
  }
  return services;
}

function requiredString(value: unknown, name: string): string {
  if (typeof value !== "string" || value.trim() === "") {
    throw new VerificationError(`${name} must be a non-empty string`);
  }
  return value;
}

export class VerificationError extends Error {
  readonly status: number;

  constructor(message: string, status = 400) {
    super(message);
    this.name = "VerificationError";
    this.status = status;
  }
}
