import assert from "node:assert/strict";
import test from "node:test";

import {
  VerificationError,
  parseAllowedServices,
  validateVerificationRequest,
} from "../src/protocol.ts";

test("accepts only report-scoped quarantine prefixes", () => {
  const allowed = parseAllowedServices("inttegro");
  assert.deepEqual(
    validateVerificationRequest(
      {
        reportId: "rpt_8eaadf11a66373a4",
        objectPrefix: "quarantine/inttegro/rpt_8eaadf11a66373a4",
      },
      allowed,
    ),
    {
      reportId: "rpt_8eaadf11a66373a4",
      objectPrefix: "quarantine/inttegro/rpt_8eaadf11a66373a4",
    },
  );
  assert.throws(
    () =>
      validateVerificationRequest(
        {
          reportId: "rpt_8eaadf11a66373a4",
          objectPrefix: "quarantine/other/rpt_8eaadf11a66373a4",
        },
        allowed,
      ),
    VerificationError,
  );
});

test("rejects invalid verifier service configuration", () => {
  assert.throws(() => parseAllowedServices("../inttegro"), /ALLOWED_SERVICES/);
});
