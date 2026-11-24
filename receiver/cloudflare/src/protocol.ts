const REPORT_ID = /^rpt_[a-z0-9_]{1,60}$/;
const CONTACT = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const SERVICE = /^[a-z][a-z0-9_-]{0,63}$/;
const SHA256 = /^[a-f0-9]{64}$/;

export const REQUIRED_ATTACHMENTS = ["bundle.json", "request.json", "service.similec"];
export const OPTIONAL_ATTACHMENTS = ["request.body"];

export type JsonObject = Record<string, unknown>;

export interface Preflight {
  simileVersion: string;
  service: string;
  serviceVersion: string;
  artifactDigest: string;
}

export interface ValidatedReport {
  value: JsonObject;
  formatVersion: number;
  id: string;
  service: string;
  serviceVersion: string;
  simileVersion: string;
  artifactDigest: string;
  fingerprint: string;
  operation: string;
  title: string;
  kind: string;
  trigger: string;
}

export function validatePreflight(value: unknown): Preflight {
  const object = objectValue(value, "preflight");
  return {
    simileVersion: stringValue(object.simileVersion, "simileVersion"),
    service: serviceValue(object.service, "service"),
    serviceVersion: stringValue(object.serviceVersion, "serviceVersion"),
    artifactDigest: digestValue(object.artifactDigest, "artifactDigest"),
  };
}

export function validateReport(value: unknown): ValidatedReport {
  const report = objectValue(value, "report");
  const verification = objectValue(report.verification, "report.verification");
  // Why: an unfamiliar report version is evidence for the independent verifier, not proof that
  // the user's report is invalid. Keep the stable intake envelope forward-compatible.
  const formatVersion = versionValue(report.formatVersion, "report.formatVersion");
  const id = stringValue(report.id, "report.id");
  if (!REPORT_ID.test(id)) {
    throw new ProtocolError("report.id is invalid");
  }
  if (verification.reproduced !== true) {
    throw new ProtocolError("report.verification.reproduced must be true");
  }
  if (report.status !== "verified") {
    throw new ProtocolError("report.status must be verified when first submitted");
  }
  if (report.reproduction !== "case.simile-repro") {
    throw new ProtocolError("report.reproduction must name case.simile-repro");
  }
  objectValue(report.diagnostic, "report.diagnostic");
  rejectContactFields(report, "report");
  return {
    value: report,
    formatVersion,
    id,
    service: serviceValue(report.service, "report.service"),
    serviceVersion: stringValue(report.serviceVersion, "report.serviceVersion"),
    simileVersion: stringValue(report.simileVersion, "report.simileVersion"),
    artifactDigest: digestValue(report.artifactDigest, "report.artifactDigest"),
    fingerprint: stringValue(verification.fingerprint, "report.verification.fingerprint"),
    operation: stringValue(report.operation, "report.operation"),
    title: stringValue(report.title, "report.title"),
    kind: stringValue(report.kind, "report.kind"),
    trigger: stringValue(report.trigger, "report.trigger"),
  };
}

export function validateReporter(value: unknown) {
  const reporter = objectValue(value, "reporter");
  const contact = objectValue(reporter.contact, "reporter.contact");
  const consent = objectValue(reporter.consent, "reporter.consent");
  const channel = stringValue(contact.channel, "reporter.contact.channel");
  const address = stringValue(contact.address, "reporter.contact.address");
  if (channel !== "email" || !CONTACT.test(address) || address.length > 254) {
    throw new ProtocolError("reporter.contact must contain a valid email address");
  }
  if (consent.clarification !== true || consent.resolutionUpdates !== true) {
    throw new ProtocolError("reporter consent must allow clarification and resolution updates");
  }
  return { channel, address, clarification: true, resolutionUpdates: true };
}

export function validateBundle(value: unknown, report: ValidatedReport): JsonObject {
  const bundle = objectValue(value, "bundle.json");
  // Why: the verifier, rather than the public ingestion boundary, owns reproduction-format
  // support. Accepting every valid version lets newer clients submit evidence without trusting it.
  versionValue(bundle.formatVersion, "bundle.formatVersion");
  if (stringValue(bundle.artifactSha256, "bundle.artifactSha256") !== report.artifactDigest) {
    throw new ProtocolError("bundle and report artifact digests differ");
  }
  if (stringValue(bundle.service, "bundle.service") !== report.service) {
    throw new ProtocolError("bundle and report service names differ");
  }
  return bundle;
}

export function reportObjectPrefix(report: Pick<ValidatedReport, "service" | "id">): string {
  // R2 keys are derived from public input, so both segments must be constrained before interpolation.
  serviceValue(report.service, "report.service");
  if (!REPORT_ID.test(report.id)) throw new ProtocolError("report.id is invalid");
  return `quarantine/${report.service}/${report.id}`;
}

export function parseAllowedServices(value: unknown): Set<string> {
  const services = new Set(
    String(value || "")
      .split(",")
      .map((service) => service.trim())
      .filter(Boolean)
      .map((service) => serviceValue(service, "ALLOWED_SERVICES")),
  );
  if (services.size === 0) {
    throw new Error("ALLOWED_SERVICES must name at least one public service");
  }
  return services;
}

export function requireAllowedService(service: string, allowedServices: ReadonlySet<string>): void {
  if (!allowedServices.has(service)) {
    throw new ProtocolError("the report service is not accepted by this receiver", 404);
  }
}

/**
 * Return true only when `candidate` is provably newer under semantic versioning.
 *
 * The receiver uses this for the Simile binary. Unknown version shapes do not
 * become upgrade advice: equality is insufficient evidence for ordering.
 */
export function isNewerVersion(candidate: unknown, current: unknown): boolean {
  const next = parseSemanticVersion(candidate);
  const installed = parseSemanticVersion(current);
  if (!next || !installed) return false;

  for (const key of ["major", "minor", "patch"] as const) {
    if (next[key] !== installed[key]) return next[key] > installed[key];
  }
  return comparePrerelease(next.prerelease, installed.prerelease) > 0;
}

export class ProtocolError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(message: string, status = 400, code = "invalid_report") {
    super(message);
    this.name = "ProtocolError";
    this.status = status;
    this.code = code;
  }
}

function objectValue(value: unknown, name: string): JsonObject {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new ProtocolError(`${name} must be an object`);
  }
  return value as JsonObject;
}

function stringValue(value: unknown, name: string): string {
  if (typeof value !== "string" || value.trim() === "") {
    throw new ProtocolError(`${name} must be a non-empty string`);
  }
  return value;
}

function versionValue(value: unknown, name: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 1) {
    throw new ProtocolError(`${name} must be a positive integer`);
  }
  return value;
}

function serviceValue(value: unknown, name: string): string {
  const service = stringValue(value, name);
  if (!SERVICE.test(service)) {
    throw new ProtocolError(`${name} must be a lowercase public service identifier`);
  }
  return service;
}

function digestValue(value: unknown, name: string): string {
  const digest = stringValue(value, name);
  if (!SHA256.test(digest)) {
    throw new ProtocolError(`${name} must be a lowercase SHA-256 digest`);
  }
  return digest;
}

function rejectContactFields(value: unknown, path: string): void {
  if (Array.isArray(value)) {
    value.forEach((item, index) => rejectContactFields(item, `${path}[${index}]`));
    return;
  }
  if (!value || typeof value !== "object") return;
  for (const [key, child] of Object.entries(value)) {
    if (["contact", "email", "address"].includes(key.toLowerCase())) {
      throw new ProtocolError(`${path}.${key} must travel in the reporter part`);
    }
    rejectContactFields(child, `${path}.${key}`);
  }
}

interface SemanticVersion {
  major: number;
  minor: number;
  patch: number;
  prerelease: string[];
}

function parseSemanticVersion(value: unknown): SemanticVersion | null {
  if (typeof value !== "string") return null;
  const match = value.match(
    /^(?:v)?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/,
  );
  if (!match) return null;
  return {
    major: Number(match[1]!),
    minor: Number(match[2]!),
    patch: Number(match[3]!),
    prerelease: match[4]?.split(".") || [],
  };
}

function comparePrerelease(left: readonly string[], right: readonly string[]): number {
  if (left.length === 0 || right.length === 0) {
    return left.length === right.length ? 0 : left.length === 0 ? 1 : -1;
  }
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    if (left[index] == null) return -1;
    if (right[index] == null) return 1;
    if (left[index] === right[index]) continue;
    const leftValue = left[index]!;
    const rightValue = right[index]!;
    const leftNumber = /^\d+$/.test(leftValue);
    const rightNumber = /^\d+$/.test(rightValue);
    if (leftNumber && rightNumber) return Number(leftValue) > Number(rightValue) ? 1 : -1;
    if (leftNumber !== rightNumber) return leftNumber ? -1 : 1;
    return leftValue > rightValue ? 1 : -1;
  }
  return 0;
}
