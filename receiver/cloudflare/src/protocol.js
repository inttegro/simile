const REPORT_ID = /^rpt_[a-z0-9_]{1,60}$/;
const CONTACT = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export const REQUIRED_ATTACHMENTS = ["bundle.json", "request.json", "service.similec"];
export const OPTIONAL_ATTACHMENTS = ["request.body"];

export function validatePreflight(value) {
  const object = objectValue(value, "preflight");
  return {
    simileVersion: stringValue(object.simileVersion, "simileVersion"),
    service: stringValue(object.service, "service"),
    serviceVersion: stringValue(object.serviceVersion, "serviceVersion"),
    artifactDigest: stringValue(object.artifactDigest, "artifactDigest"),
  };
}

export function validateReport(value) {
  const report = objectValue(value, "report");
  const verification = objectValue(report.verification, "report.verification");
  if (report.formatVersion !== 1) {
    throw new ProtocolError("report.formatVersion must be 1");
  }
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
    id,
    service: stringValue(report.service, "report.service"),
    serviceVersion: stringValue(report.serviceVersion, "report.serviceVersion"),
    simileVersion: stringValue(report.simileVersion, "report.simileVersion"),
    artifactDigest: stringValue(report.artifactDigest, "report.artifactDigest"),
    fingerprint: stringValue(verification.fingerprint, "report.verification.fingerprint"),
  };
}

export function validateReporter(value) {
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

export function validateBundle(value, report) {
  const bundle = objectValue(value, "bundle.json");
  if (bundle.formatVersion !== 1 && bundle.formatVersion !== 2) {
    throw new ProtocolError("bundle.json formatVersion is unsupported");
  }
  if (stringValue(bundle.artifactSha256, "bundle.artifactSha256") !== report.artifactDigest) {
    throw new ProtocolError("bundle and report artifact digests differ");
  }
  if (stringValue(bundle.service, "bundle.service") !== report.service) {
    throw new ProtocolError("bundle and report service names differ");
  }
  return bundle;
}

export function reportObjectPrefix(report) {
  return `quarantine/${report.service}/${report.id}`;
}

/**
 * Return true only when `candidate` is provably newer under semantic versioning.
 *
 * The receiver uses this for the Simile binary. Unknown version shapes do not
 * become upgrade advice: equality is insufficient evidence for ordering.
 */
export function isNewerVersion(candidate, current) {
  const next = parseSemanticVersion(candidate);
  const installed = parseSemanticVersion(current);
  if (!next || !installed) return false;

  for (const key of ["major", "minor", "patch"]) {
    if (next[key] !== installed[key]) return next[key] > installed[key];
  }
  return comparePrerelease(next.prerelease, installed.prerelease) > 0;
}

export class ProtocolError extends Error {
  constructor(message, status = 400) {
    super(message);
    this.name = "ProtocolError";
    this.status = status;
  }
}

function objectValue(value, name) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new ProtocolError(`${name} must be an object`);
  }
  return value;
}

function stringValue(value, name) {
  if (typeof value !== "string" || value.trim() === "") {
    throw new ProtocolError(`${name} must be a non-empty string`);
  }
  return value;
}

function rejectContactFields(value, path) {
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

function parseSemanticVersion(value) {
  if (typeof value !== "string") return null;
  const match = value.match(
    /^(?:v)?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/,
  );
  if (!match) return null;
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    prerelease: match[4]?.split(".") || [],
  };
}

function comparePrerelease(left, right) {
  if (left.length === 0 || right.length === 0) {
    return left.length === right.length ? 0 : left.length === 0 ? 1 : -1;
  }
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    if (left[index] == null) return -1;
    if (right[index] == null) return 1;
    if (left[index] === right[index]) continue;
    const leftNumber = /^\d+$/.test(left[index]);
    const rightNumber = /^\d+$/.test(right[index]);
    if (leftNumber && rightNumber) return Number(left[index]) > Number(right[index]) ? 1 : -1;
    if (leftNumber !== rightNumber) return leftNumber ? -1 : 1;
    return left[index] > right[index] ? 1 : -1;
  }
  return 0;
}
