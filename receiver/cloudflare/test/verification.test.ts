import assert from "node:assert/strict";
import test from "node:test";

import { decideVerification } from "../src/verification.ts";

const fingerprint = "abc123:createOrder:failure";

test("accepts only a conclusive reproduction with the expected fingerprint", () => {
  assert.deepEqual(
    decideVerification({ conclusive: true, reproduced: true, fingerprint }, fingerprint),
    { status: "accepted", fingerprint, reason: null, reasonCode: null },
  );
  assert.equal(
    decideVerification(
      { conclusive: true, reproduced: true, fingerprint: "different" },
      fingerprint,
    ).status,
    "rejected",
  );
});

test("keeps evidence pending when the verifier cannot understand or run it", () => {
  const decision = decideVerification(
    {
      conclusive: false,
      reproduced: false,
      fingerprint: null,
      reasonCode: "verifier_could_not_prove",
      reason: "unsupported reproduction bundle format 42",
    },
    fingerprint,
  );
  assert.equal(decision.status, "pending_verification");
  assert.equal(decision.reasonCode, "verifier_could_not_prove");
});

test("rejects a conclusive non-reproduction", () => {
  const decision = decideVerification(
    {
      conclusive: true,
      reproduced: false,
      fingerprint: null,
      reasonCode: "not_reproduced",
      reason: "the recorded behavior did not reproduce",
    },
    fingerprint,
  );
  assert.equal(decision.status, "rejected");
  assert.equal(decision.reasonCode, "not_reproduced");
});
