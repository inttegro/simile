import assert from "node:assert/strict";
import test from "node:test";

import {
  ProtocolError,
  isNewerVersion,
  reportObjectPrefix,
  validateBundle,
  validatePreflight,
  validateReport,
  validateReporter,
} from "../src/protocol.js";

function report() {
  return {
    formatVersion: 1,
    id: "rpt_8eaadf11a66373a4",
    service: "inttegro",
    serviceVersion: "2026-11-22",
    simileVersion: "0.3.0",
    artifactDigest: "abc123",
    status: "verified",
    reproduction: "case.simile-repro",
    diagnostic: { code: "emulation_inconsistency", message: "response mismatch" },
    verification: { reproduced: true, fingerprint: "abc123:createOrder:failure" },
  };
}

test("validates the report and keeps its quarantine prefix predictable", () => {
  const validated = validateReport(report());
  assert.equal(validated.id, "rpt_8eaadf11a66373a4");
  assert.equal(reportObjectPrefix(validated), "quarantine/inttegro/rpt_8eaadf11a66373a4");
});

test("rejects contact data embedded in the technical report", () => {
  const value = report();
  value.reporter = { email: "person@example.com" };
  assert.throws(() => validateReport(value), ProtocolError);
});

test("requires explicit reporter consent and a usable email", () => {
  const reporter = validateReporter({
    contact: { channel: "email", address: "person@example.com" },
    consent: { clarification: true, resolutionUpdates: true },
  });
  assert.equal(reporter.address, "person@example.com");
  assert.throws(
    () =>
      validateReporter({
        contact: { channel: "email", address: "invalid" },
        consent: { clarification: true, resolutionUpdates: true },
      }),
    ProtocolError,
  );
});

test("requires the bundle to agree with the report", () => {
  const validated = validateReport(report());
  assert.doesNotThrow(() =>
    validateBundle(
      { formatVersion: 2, artifactSha256: "abc123", service: "inttegro" },
      validated,
    ),
  );
  assert.throws(
    () =>
      validateBundle(
        { formatVersion: 2, artifactSha256: "different", service: "inttegro" },
        validated,
      ),
    ProtocolError,
  );
});

test("validates preflight identity", () => {
  assert.deepEqual(
    validatePreflight({
      simileVersion: "0.3.0",
      service: "inttegro",
      serviceVersion: "2026-11-22",
      artifactDigest: "abc123",
    }),
    {
      simileVersion: "0.3.0",
      service: "inttegro",
      serviceVersion: "2026-11-22",
      artifactDigest: "abc123",
    },
  );
});

test("recommends only a provably newer Simile release", () => {
  assert.equal(isNewerVersion("0.4.0", "0.3.9"), true);
  assert.equal(isNewerVersion("0.4.0", "0.4.0-rc.1"), true);
  assert.equal(isNewerVersion("0.3.9", "0.4.0"), false);
  assert.equal(isNewerVersion("nightly", "0.4.0"), false);
});
