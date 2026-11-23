import assert from "node:assert/strict";
import test from "node:test";

import { decryptContact } from "../src/api.ts";

function base64(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("base64");
}

test("decrypts reporter contact only with the report-bound key material", async () => {
  const reportId = "rpt_8eaadf11a66373a4";
  const keyBytes = crypto.getRandomValues(new Uint8Array(32));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await crypto.subtle.importKey("raw", keyBytes, "AES-GCM", false, ["encrypt"]);
  const ciphertext = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv, additionalData: new TextEncoder().encode(reportId) },
    key,
    new TextEncoder().encode("developer@example.com"),
  );

  assert.equal(
    await decryptContact(base64(keyBytes), reportId, base64(new Uint8Array(ciphertext)), base64(iv)),
    "developer@example.com",
  );
  await assert.rejects(() =>
    decryptContact(base64(keyBytes), "rpt_other", base64(new Uint8Array(ciphertext)), base64(iv)),
  );
});
