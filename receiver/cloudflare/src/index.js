import {
  OPTIONAL_ATTACHMENTS,
  ProtocolError,
  REQUIRED_ATTACHMENTS,
  isNewerVersion,
  reportObjectPrefix,
  validateBundle,
  validatePreflight,
  validateReport,
  validateReporter,
} from "./protocol.js";

const JSON_HEADERS = { "content-type": "application/json; charset=utf-8" };

export default {
  async fetch(request, env) {
    try {
      const url = new URL(request.url);
      if (request.method === "POST" && url.pathname === "/v1/reports/preflight") {
        return await preflight(request, env);
      }
      if (request.method === "POST" && url.pathname === "/v1/reports") {
        return await receive(request, env);
      }
      return json({ error: { code: "not_found", message: "route not found" } }, 404);
    } catch (error) {
      if (error instanceof ProtocolError) {
        return json({ error: { code: "invalid_report", message: error.message } }, error.status);
      }
      console.error("report receiver failed", error);
      return json(
        { error: { code: "receiver_failed", message: "the report receiver could not complete the request" } },
        500,
      );
    }
  },

  async queue(batch, env) {
    for (const message of batch.messages) {
      try {
        await verify(message.body, env);
        message.ack();
      } catch (error) {
        console.error("report verification failed to run", error);
        message.retry({ delaySeconds: 60 });
      }
    }
  },
};

async function preflight(request, env) {
  requireJson(request);
  const input = validatePreflight(await request.json());
  const [simile, service] = await Promise.all([
    env.DB.prepare(
      "SELECT version, command FROM simile_releases WHERE current = 1 LIMIT 1",
    ).first(),
    env.DB.prepare(
      "SELECT version, artifact_digest, command FROM service_releases WHERE service = ? AND current = 1 LIMIT 1",
    )
      .bind(input.service)
      .first(),
  ]);
  const recommendations = [];
  if (simile && isNewerVersion(simile.version, input.simileVersion)) {
    recommendations.push({
      component: "simile",
      current: input.simileVersion,
      available: simile.version,
      command: simile.command,
    });
  }
  if (
    service &&
    (service.version !== input.serviceVersion ||
      (service.artifact_digest && service.artifact_digest !== input.artifactDigest))
  ) {
    recommendations.push({
      component: "service",
      current: input.serviceVersion,
      available: service.version,
      command: service.command,
    });
  }
  return json({ recommendations });
}

async function receive(request, env) {
  const limit = Number.parseInt(env.MAX_REPORT_BYTES || "8388608", 10);
  const declared = Number.parseInt(request.headers.get("content-length") || "0", 10);
  if (!Number.isSafeInteger(declared) || declared <= 0) {
    throw new ProtocolError("content-length is required", 411);
  }
  if (declared > limit) throw new ProtocolError(`report exceeds the ${limit} byte limit`, 413);
  if (!request.headers.get("content-type")?.startsWith("multipart/form-data")) {
    throw new ProtocolError("content-type must be multipart/form-data", 415);
  }
  const form = await request.formData();
  const report = validateReport(await parseJsonPart(form, "report"));
  const reporter = validateReporter(await parseJsonPart(form, "reporter"));
  const attachments = await readAttachments(form, limit);
  validateBundle(parseAttachmentJson(attachments, "bundle.json"), report);
  parseAttachmentJson(attachments, "request.json");

  const existing = await env.DB.prepare(
    "SELECT artifact_digest, fingerprint, status FROM reports WHERE id = ?",
  )
    .bind(report.id)
    .first();
  if (existing) {
    if (
      existing.artifact_digest !== report.artifactDigest ||
      existing.fingerprint !== report.fingerprint
    ) {
      throw new ProtocolError("report identifier already belongs to different evidence", 409);
    }
    await env.VERIFICATION_QUEUE.send({ reportId: report.id });
    return json({ id: report.id, status: existing.status });
  }

  const prefix = reportObjectPrefix(report);
  const written = [];
  let recorded = false;
  try {
    await put(env.EVIDENCE, `${prefix}/report.json`, JSON.stringify(report.value), "application/json", written);
    for (const [name, bytes] of attachments) {
      const type = name.endsWith(".json") ? "application/json" : "application/octet-stream";
      await put(env.EVIDENCE, `${prefix}/${name}`, bytes, type, written);
    }
    const encrypted = await encryptContact(env.REPORTER_CONTACT_KEY, report.id, reporter.address);
    const now = new Date().toISOString();
    await env.DB.batch([
      env.DB.prepare(
        `INSERT INTO reports
          (id, status, service, service_version, simile_version, artifact_digest, fingerprint,
           object_prefix, created_at, updated_at)
         VALUES (?, 'pending_verification', ?, ?, ?, ?, ?, ?, ?, ?)`,
      ).bind(
        report.id,
        report.service,
        report.serviceVersion,
        report.simileVersion,
        report.artifactDigest,
        report.fingerprint,
        prefix,
        now,
        now,
      ),
      env.DB.prepare(
        `INSERT INTO reporter_contacts
          (report_id, channel, ciphertext, iv, clarification, resolution_updates, created_at)
         VALUES (?, ?, ?, ?, 1, 1, ?)`,
      ).bind(report.id, reporter.channel, encrypted.ciphertext, encrypted.iv, now),
    ]);
    recorded = true;
    await env.VERIFICATION_QUEUE.send({ reportId: report.id });
    return json({ id: report.id, status: "pending_verification" }, 202);
  } catch (error) {
    if (!recorded) {
      await Promise.allSettled(written.map((key) => env.EVIDENCE.delete(key)));
    }
    throw error;
  }
}

async function verify(message, env) {
  const reportId = message?.reportId;
  if (typeof reportId !== "string") throw new Error("verification message has no reportId");
  const report = await env.DB.prepare(
    "SELECT object_prefix, fingerprint, status FROM reports WHERE id = ?",
  )
    .bind(reportId)
    .first();
  if (!report || report.status === "accepted" || report.status === "rejected") return;
  const response = await env.VERIFIER.fetch("https://simile-verifier.internal/v1/verify", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ reportId, objectPrefix: report.object_prefix }),
  });
  if (!response.ok) throw new Error(`verifier returned HTTP ${response.status}`);
  const result = await response.json();
  const accepted = result.reproduced === true && result.fingerprint === report.fingerprint;
  const status = accepted ? "accepted" : "rejected";
  const reason = accepted ? null : String(result.reason || "reproduction did not match");
  await env.DB.prepare(
    "UPDATE reports SET status = ?, rejection_reason = ?, updated_at = ? WHERE id = ?",
  )
    .bind(status, reason, new Date().toISOString(), reportId)
    .run();
}

function parseJsonPart(form, name) {
  const value = form.get(name);
  if (!(value instanceof File)) throw new ProtocolError(`${name} must be a JSON file part`);
  return value.text().then((text) => {
    try {
      return JSON.parse(text);
    } catch {
      throw new ProtocolError(`${name} must contain valid JSON`);
    }
  });
}

async function readAttachments(form, limit) {
  const attachments = new Map();
  let bytes = 0;
  for (const name of [...REQUIRED_ATTACHMENTS, ...OPTIONAL_ATTACHMENTS]) {
    const value = form.get(name);
    if (value == null && OPTIONAL_ATTACHMENTS.includes(name)) continue;
    if (!(value instanceof File)) throw new ProtocolError(`${name} file part is required`);
    const content = new Uint8Array(await value.arrayBuffer());
    bytes += content.byteLength;
    if (bytes > limit) throw new ProtocolError(`report exceeds the ${limit} byte limit`, 413);
    attachments.set(name, content);
  }
  return attachments;
}

function parseAttachmentJson(attachments, name) {
  try {
    return JSON.parse(new TextDecoder().decode(attachments.get(name)));
  } catch {
    throw new ProtocolError(`${name} must contain valid JSON`);
  }
}

async function put(bucket, key, value, contentType, written) {
  await bucket.put(key, value, { httpMetadata: { contentType } });
  written.push(key);
}

async function encryptContact(encodedKey, reportId, contact) {
  if (!encodedKey) throw new Error("REPORTER_CONTACT_KEY is not configured");
  const keyBytes = Uint8Array.from(atob(encodedKey), (character) => character.charCodeAt(0));
  if (keyBytes.byteLength !== 32) throw new Error("REPORTER_CONTACT_KEY must decode to 32 bytes");
  const key = await crypto.subtle.importKey("raw", keyBytes, "AES-GCM", false, ["encrypt"]);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv, additionalData: new TextEncoder().encode(reportId) },
    key,
    new TextEncoder().encode(contact),
  );
  return { ciphertext: base64(new Uint8Array(ciphertext)), iv: base64(iv) };
}

function base64(bytes) {
  let value = "";
  for (const byte of bytes) value += String.fromCharCode(byte);
  return btoa(value);
}

function requireJson(request) {
  if (!request.headers.get("content-type")?.startsWith("application/json")) {
    throw new ProtocolError("content-type must be application/json", 415);
  }
}

function json(value, status = 200) {
  return new Response(JSON.stringify(value), { status, headers: JSON_HEADERS });
}
