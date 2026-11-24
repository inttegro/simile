import {
  OPTIONAL_ATTACHMENTS,
  ProtocolError,
  REQUIRED_ATTACHMENTS,
  isNewerVersion,
  parseAllowedServices,
  reportObjectPrefix,
  requireAllowedService,
  validateBundle,
  validatePreflight,
  validateReport,
  validateReporter,
  type ValidatedReport,
} from "./protocol.ts";
import { decideVerification } from "./verification.ts";

interface ReceiverEnv {
  DB: D1Database;
  EVIDENCE: R2Bucket;
  VERIFICATION_QUEUE: Queue<VerificationMessage>;
  REPORT_RATE_LIMITER: RateLimit;
  VERIFIER: Fetcher;
  ALLOWED_SERVICES: string;
  REPORTER_CONTACT_KEY: string;
  REPORTER_CONTACT_KEY_VERSION?: string;
  MAX_REPORT_BYTES?: string;
  EVIDENCE_RETENTION_DAYS?: string;
  QUARANTINE_RETENTION_DAYS?: string;
  CONTACT_RETENTION_DAYS?: string;
}

interface VerificationMessage {
  reportId: string;
}

interface ReleaseRow {
  version: string;
  command: string;
  artifact_digest?: string | null;
}

interface StoredReportRow {
  artifact_digest: string;
  fingerprint: string;
  operation: string;
  service: string;
  service_version: string;
  status: string;
}

const JSON_HEADERS = {
  "cache-control": "no-store",
  "content-security-policy": "default-src 'none'; frame-ancestors 'none'",
  "content-type": "application/json; charset=utf-8",
  "referrer-policy": "no-referrer",
  "x-content-type-options": "nosniff",
};
const REPORT_PARTS = new Set([
  "report",
  "reporter",
  ...REQUIRED_ATTACHMENTS,
  ...OPTIONAL_ATTACHMENTS,
]);

export default {
  async fetch(request: Request, env: ReceiverEnv): Promise<Response> {
    try {
      const url = new URL(request.url);
      if (request.method === "GET" && url.pathname === "/healthz") {
        return await health(env);
      }
      if (request.method === "POST" && url.pathname === "/v1/reports/preflight") {
        await enforceRateLimit(request, env, 1);
        return await preflight(request, env);
      }
      if (request.method === "POST" && url.pathname === "/v1/reports") {
        await enforceRateLimit(request, env, 10);
        return await receive(request, env);
      }
      return json({ error: { code: "not_found", message: "route not found" } }, 404);
    } catch (error) {
      if (error instanceof ProtocolError) {
        return json({ error: { code: error.code, message: error.message } }, error.status);
      }
      console.error("report receiver failed", safeError(error));
      return json(
        { error: { code: "receiver_failed", message: "the report receiver could not complete the request" } },
        500,
      );
    }
  },

  async queue(batch: MessageBatch<VerificationMessage>, env: ReceiverEnv): Promise<void> {
    for (const message of batch.messages) {
      try {
        await verify(message.body, env);
        message.ack();
      } catch (error) {
        console.error("report verification failed to run", safeError(error));
        message.retry({ delaySeconds: 60 });
      }
    }
  },

  async scheduled(_controller: ScheduledController, env: ReceiverEnv): Promise<void> {
    await applyRetention(env);
    await requeueDeferredVerifications(env);
  },
};

async function health(env: ReceiverEnv): Promise<Response> {
  const required = ["DB", "EVIDENCE", "VERIFICATION_QUEUE", "REPORT_RATE_LIMITER", "VERIFIER"] as const;
  const missing: string[] = required.filter((binding) => !env[binding]);
  if (!env.ALLOWED_SERVICES) missing.push("ALLOWED_SERVICES");
  if (!env.REPORTER_CONTACT_KEY) missing.push("REPORTER_CONTACT_KEY");
  if (missing.length > 0) return json({ status: "misconfigured", missing }, 503);
  parseAllowedServices(env.ALLOWED_SERVICES);
  await env.DB.prepare("SELECT 1 AS ready").first();
  return json({ status: "ok" });
}

async function preflight(request: Request, env: ReceiverEnv): Promise<Response> {
  requireJson(request);
  const input = validatePreflight(await request.json());
  requireAllowedService(input.service, parseAllowedServices(env.ALLOWED_SERVICES));
  const [simile, service] = await Promise.all([
    env.DB.prepare(
      "SELECT version, command FROM simile_releases WHERE current = 1 LIMIT 1",
    ).first<ReleaseRow>(),
    env.DB.prepare(
      "SELECT version, artifact_digest, command FROM service_releases WHERE service = ? AND current = 1 LIMIT 1",
    )
      .bind(input.service)
      .first<ReleaseRow>(),
  ]);
  const recommendations: Array<{
    component: "simile" | "service";
    current: string;
    available: string;
    command: string;
  }> = [];
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

async function receive(request: Request, env: ReceiverEnv): Promise<Response> {
  const limit = positiveInteger(env.MAX_REPORT_BYTES, 8 * 1024 * 1024);
  const declared = Number.parseInt(request.headers.get("content-length") || "0", 10);
  if (!Number.isSafeInteger(declared) || declared <= 0) {
    throw new ProtocolError("content-length is required", 411, "length_required");
  }
  if (declared > limit) {
    throw new ProtocolError(`report exceeds the ${limit} byte limit`, 413, "report_too_large");
  }
  if (!request.headers.get("content-type")?.startsWith("multipart/form-data")) {
    throw new ProtocolError("content-type must be multipart/form-data", 415, "unsupported_media_type");
  }

  const form = await request.formData();
  validatePartNames(form);
  const report = validateReport(await parseJsonPart(form, "report"));
  requireAllowedService(report.service, parseAllowedServices(env.ALLOWED_SERVICES));
  await requireKnownArtifact(report, env);
  const reporter = validateReporter(await parseJsonPart(form, "reporter"));
  const attachments = await readAttachments(form, limit);
  const bundle = validateBundle(parseAttachmentJson(attachments, "bundle.json"), report);
  parseAttachmentJson(attachments, "request.json");

  const existing = await env.DB.prepare(
    `SELECT artifact_digest, fingerprint, operation, service, service_version, status
       FROM reports WHERE id = ?`,
  )
    .bind(report.id)
    .first<StoredReportRow>();
  if (existing) {
    if (
      existing.artifact_digest !== report.artifactDigest ||
      existing.fingerprint !== report.fingerprint ||
      existing.operation !== report.operation ||
      existing.service !== report.service ||
      existing.service_version !== report.serviceVersion
    ) {
      throw new ProtocolError(
        "report identifier already belongs to different evidence",
        409,
        "report_id_conflict",
      );
    }
    if (existing.status === "pending_verification") {
      await env.VERIFICATION_QUEUE.send({ reportId: report.id });
    }
    return json({ id: report.id, status: submissionStatus(existing.status) });
  }

  const prefix = reportObjectPrefix(report);
  const written: string[] = [];
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
           object_prefix, created_at, updated_at, operation, title, kind, trigger)
         VALUES (?, 'pending_verification', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
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
        report.operation,
        report.title,
        report.kind,
        report.trigger,
      ),
      env.DB.prepare(
        `INSERT INTO reporter_contacts
          (report_id, channel, ciphertext, iv, clarification, resolution_updates, created_at, key_version)
         VALUES (?, ?, ?, ?, 1, 1, ?, ?)`,
      ).bind(
        report.id,
        reporter.channel,
        encrypted.ciphertext,
        encrypted.iv,
        now,
        env.REPORTER_CONTACT_KEY_VERSION || "v1",
      ),
      env.DB.prepare(
        `INSERT INTO report_events (id, report_id, kind, actor, detail, created_at)
         VALUES (?, ?, 'received', 'receiver', ?, ?)`,
      ).bind(
        crypto.randomUUID(),
        report.id,
        JSON.stringify({
          serviceVersion: report.serviceVersion,
          reportFormatVersion: report.formatVersion,
          bundleFormatVersion: bundle.formatVersion,
        }),
        now,
      ),
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

async function verify(message: VerificationMessage, env: ReceiverEnv): Promise<void> {
  const reportId = message?.reportId;
  if (typeof reportId !== "string") throw new Error("verification message has no reportId");
  const report = await env.DB.prepare(
    "SELECT object_prefix, fingerprint, status FROM reports WHERE id = ?",
  )
    .bind(reportId)
    .first<{ object_prefix: string; fingerprint: string; status: string }>();
  if (!report || ["accepted", "rejected", "duplicate", "resolved"].includes(report.status)) return;
  const response = await env.VERIFIER.fetch("https://simile-verifier.internal/v1/verify", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ reportId, objectPrefix: report.object_prefix }),
  });
  if (!response.ok) throw new Error(`verifier returned HTTP ${response.status}`);
  const decision = decideVerification(await response.json(), report.fingerprint);
  const now = new Date().toISOString();
  if (decision.status === "pending_verification") {
    // Why: inability to understand a newer format is a verifier limitation, not evidence that the
    // reporter is wrong. Keep the quarantined case eligible for a future verifier retry.
    await env.DB.batch([
      env.DB.prepare(
        `UPDATE reports
            SET status = 'pending_verification', rejection_reason = NULL,
                verification_reproduced = NULL, verification_fingerprint = ?, updated_at = ?
          WHERE id = ?`,
      ).bind(decision.fingerprint, now, reportId),
      env.DB.prepare(
        `INSERT INTO report_events (id, report_id, kind, actor, detail, created_at)
         VALUES (?, ?, 'verification_deferred', 'verifier', ?, ?)`,
      ).bind(
        crypto.randomUUID(),
        reportId,
        JSON.stringify({ reason: decision.reason, reasonCode: decision.reasonCode }),
        now,
      ),
    ]);
    return;
  }
  const accepted = decision.status === "accepted";
  const status = decision.status;
  const reason = decision.reason;
  await env.DB.batch([
    env.DB.prepare(
      `UPDATE reports
          SET status = ?, rejection_reason = ?, verification_reproduced = ?,
              verification_fingerprint = ?, verified_at = ?, updated_at = ?
        WHERE id = ?`,
    ).bind(
      status,
      reason,
      accepted ? 1 : 0,
      decision.fingerprint,
      now,
      now,
      reportId,
    ),
    env.DB.prepare(
      `INSERT INTO report_events (id, report_id, kind, actor, detail, created_at)
       VALUES (?, ?, ?, 'verifier', ?, ?)`,
    ).bind(
      crypto.randomUUID(),
      reportId,
      accepted ? "verification_accepted" : "verification_rejected",
      JSON.stringify({ reason, reasonCode: decision.reasonCode }),
      now,
    ),
  ]);
}

async function requireKnownArtifact(report: ValidatedReport, env: ReceiverEnv): Promise<void> {
  const release = await env.DB.prepare(
    `SELECT 1 AS known FROM service_releases
      WHERE service = ? AND version = ? AND artifact_digest = ? LIMIT 1`,
  )
    .bind(report.service, report.serviceVersion, report.artifactDigest)
    .first();
  if (!release) {
    throw new ProtocolError(
      "the report does not use a published service artifact",
      422,
      "unknown_service_artifact",
    );
  }
}

async function enforceRateLimit(request: Request, env: ReceiverEnv, weight: number): Promise<void> {
  if (!env.REPORT_RATE_LIMITER) throw new Error("REPORT_RATE_LIMITER is not configured");
  const client = request.headers.get("cf-connecting-ip") || "unattributed";
  const result = await env.REPORT_RATE_LIMITER.limit({ key: client, delta: weight });
  if (!result.success) {
    throw new ProtocolError("report submission rate exceeded", 429, "rate_limited");
  }
}

async function applyRetention(env: ReceiverEnv): Promise<void> {
  const now = Date.now();
  const evidenceCutoff = new Date(
    now - positiveInteger(env.EVIDENCE_RETENTION_DAYS, 180) * 86_400_000,
  ).toISOString();
  const quarantineCutoff = new Date(
    now - positiveInteger(env.QUARANTINE_RETENTION_DAYS, 30) * 86_400_000,
  ).toISOString();
  const contactCutoff = new Date(
    now - positiveInteger(env.CONTACT_RETENTION_DAYS, 90) * 86_400_000,
  ).toISOString();

  const expiring = await env.DB.prepare(
    `SELECT id, object_prefix FROM reports
      WHERE evidence_deleted_at IS NULL
        AND ((status IN ('pending_verification', 'rejected', 'duplicate') AND created_at < ?)
          OR created_at < ?)
      ORDER BY created_at ASC LIMIT 100`,
  )
    .bind(quarantineCutoff, evidenceCutoff)
    .all<{ id: string; object_prefix: string }>();
  for (const report of expiring.results || []) {
    await deletePrefix(env.EVIDENCE, report.object_prefix);
    const deletedAt = new Date().toISOString();
    await env.DB.batch([
      env.DB.prepare(
        "UPDATE reports SET evidence_deleted_at = ?, updated_at = ? WHERE id = ?",
      ).bind(deletedAt, deletedAt, report.id),
      env.DB.prepare(
        `INSERT INTO report_events (id, report_id, kind, actor, detail, created_at)
         VALUES (?, ?, 'evidence_expired', 'retention', '{}', ?)`,
      ).bind(crypto.randomUUID(), report.id, deletedAt),
    ]);
  }

  const expiredContacts = await env.DB.prepare(
    "SELECT report_id FROM reporter_contacts WHERE created_at < ? LIMIT 100",
  )
    .bind(contactCutoff)
    .all<{ report_id: string }>();
  for (const contact of expiredContacts.results || []) {
    const deletedAt = new Date().toISOString();
    await env.DB.batch([
      env.DB.prepare("DELETE FROM reporter_contacts WHERE report_id = ?").bind(contact.report_id),
      env.DB.prepare(
        "UPDATE reports SET contact_deleted_at = ?, updated_at = ? WHERE id = ?",
      ).bind(deletedAt, deletedAt, contact.report_id),
      env.DB.prepare(
        `INSERT INTO report_events (id, report_id, kind, actor, detail, created_at)
         VALUES (?, ?, 'contact_expired', 'retention', '{}', ?)`,
      ).bind(crypto.randomUUID(), contact.report_id, deletedAt),
    ]);
  }
}

async function requeueDeferredVerifications(env: ReceiverEnv): Promise<void> {
  const retryBefore = new Date(Date.now() - 23 * 60 * 60 * 1000).toISOString();
  const pending = await env.DB.prepare(
    `SELECT id FROM reports
      WHERE status = 'pending_verification' AND evidence_deleted_at IS NULL AND updated_at < ?
      ORDER BY updated_at ASC LIMIT 100`,
  )
    .bind(retryBefore)
    .all<{ id: string }>();
  for (const report of pending.results || []) {
    // Why: a report deferred by an older verifier must be retried automatically after verifier
    // upgrades; requiring the reporter to resubmit would punish them for being ahead of us.
    await env.VERIFICATION_QUEUE.send({ reportId: report.id });
    const now = new Date().toISOString();
    await env.DB.batch([
      env.DB.prepare("UPDATE reports SET updated_at = ? WHERE id = ?").bind(now, report.id),
      env.DB.prepare(
        `INSERT INTO report_events (id, report_id, kind, actor, detail, created_at)
         VALUES (?, ?, 'verification_requeued', 'receiver', '{}', ?)`,
      ).bind(crypto.randomUUID(), report.id, now),
    ]);
  }
}

function validatePartNames(form: FormData): void {
  const counts = new Map<string, number>();
  for (const key of form.keys()) {
    if (!REPORT_PARTS.has(key)) throw new ProtocolError(`unexpected multipart part ${key}`);
    counts.set(key, (counts.get(key) || 0) + 1);
    if ((counts.get(key) ?? 0) > 1) {
      throw new ProtocolError(`multipart part ${key} must appear once`);
    }
  }
}

async function parseJsonPart(form: FormData, name: string): Promise<unknown> {
  const value = form.get(name);
  if (!(value instanceof File)) throw new ProtocolError(`${name} must be a JSON file part`);
  if (value.size > 64 * 1024) throw new ProtocolError(`${name} exceeds the 65536 byte limit`, 413);
  const text = await value.text();
  try {
    return JSON.parse(text);
  } catch {
    throw new ProtocolError(`${name} must contain valid JSON`);
  }
}

async function readAttachments(form: FormData, limit: number): Promise<Map<string, Uint8Array>> {
  const attachments = new Map<string, Uint8Array>();
  let bytes = 0;
  for (const name of [...REQUIRED_ATTACHMENTS, ...OPTIONAL_ATTACHMENTS]) {
    const value = form.get(name);
    if (value == null && OPTIONAL_ATTACHMENTS.includes(name)) continue;
    if (!(value instanceof File)) throw new ProtocolError(`${name} file part is required`);
    const content = new Uint8Array(await value.arrayBuffer());
    bytes += content.byteLength;
    if (bytes > limit) {
      throw new ProtocolError(`report exceeds the ${limit} byte limit`, 413, "report_too_large");
    }
    attachments.set(name, content);
  }
  return attachments;
}

function parseAttachmentJson(attachments: ReadonlyMap<string, Uint8Array>, name: string): unknown {
  const bytes = attachments.get(name);
  if (!bytes) throw new ProtocolError(`${name} file part is required`);
  try {
    return JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    throw new ProtocolError(`${name} must contain valid JSON`);
  }
}

async function put(
  bucket: R2Bucket,
  key: string,
  value: string | Uint8Array,
  contentType: string,
  written: string[],
): Promise<void> {
  await bucket.put(key, value, { httpMetadata: { contentType } });
  written.push(key);
}

async function deletePrefix(bucket: R2Bucket, prefix: string): Promise<void> {
  let cursor: string | undefined;
  do {
    const page: R2Objects = await bucket.list({
      prefix: `${prefix}/`,
      ...(cursor ? { cursor } : {}),
      limit: 1000,
    });
    if (page.objects.length > 0) await bucket.delete(page.objects.map((object) => object.key));
    cursor = page.truncated ? page.cursor : undefined;
  } while (cursor);
}

async function encryptContact(
  encodedKey: string,
  reportId: string,
  contact: string,
): Promise<{ ciphertext: string; iv: string }> {
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

function base64(bytes: Uint8Array): string {
  let value = "";
  for (const byte of bytes) value += String.fromCharCode(byte);
  return btoa(value);
}

function requireJson(request: Request): void {
  if (!request.headers.get("content-type")?.startsWith("application/json")) {
    throw new ProtocolError(
      "content-type must be application/json",
      415,
      "unsupported_media_type",
    );
  }
}

function submissionStatus(status: string): "accepted" | "pending_verification" {
  return status === "accepted" ? "accepted" : "pending_verification";
}

function positiveInteger(value: string | undefined, fallback: number): number {
  const parsed = Number.parseInt(value || String(fallback), 10);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : fallback;
}

function safeError(error: unknown): { name?: string; message: string } {
  return error instanceof Error ? { name: error.name, message: error.message } : { message: "unknown error" };
}

function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), { status, headers: JSON_HEADERS });
}
