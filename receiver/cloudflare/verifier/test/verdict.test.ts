import assert from "node:assert/strict";
import test from "node:test";

import { classifyRunResult } from "../container/verdict.ts";

test("classifies a successful independent reproduction as conclusive", () => {
  const verdict = classifyRunResult({
    exitCode: 0,
    stdout: JSON.stringify({
      report: { verification: { reproduced: true, fingerprint: "proof-fingerprint" } },
    }),
    stderr: "",
  });
  assert.deepEqual(verdict, {
    conclusive: true,
    reproduced: true,
    fingerprint: "proof-fingerprint",
    reason: null,
    reasonCode: null,
  });
});

test("defers bundle formats the installed verifier cannot prove", () => {
  const verdict = classifyRunResult({
    exitCode: 1,
    stdout: "",
    stderr: "unsupported reproduction bundle format 42; supported format is 3\n",
  });
  assert.equal(verdict.conclusive, false);
  assert.equal(verdict.reasonCode, "verifier_could_not_prove");
});

test("does not reject a report when the verifier returns incomplete proof output", () => {
  const verdict = classifyRunResult({
    exitCode: 0,
    stdout: JSON.stringify({ report: { verification: { reproduced: true } } }),
    stderr: "",
  });
  assert.equal(verdict.conclusive, false);
  assert.equal(verdict.reasonCode, "verifier_could_not_prove");
});

test("distinguishes a deterministic mismatch from verifier incompatibility", () => {
  const verdict = classifyRunResult({
    exitCode: 1,
    stdout: "",
    stderr: "reproduction bundle `/tmp/case.simile-repro` did not reproduce its recorded behavior\n",
  });
  assert.equal(verdict.conclusive, true);
  assert.equal(verdict.reproduced, false);
  assert.equal(verdict.reasonCode, "not_reproduced");
});
