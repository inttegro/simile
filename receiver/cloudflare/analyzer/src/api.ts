const REPORT_ID = /^rpt_[a-z0-9_]{1,60}$/;
const EVIDENCE_NAMES = new Set([
  "bundle.json",
  "report.json",
  "request.body",
  "request.json",
  "service.similec",
]);
const REPORT_STATUSES = new Set([
  "accepted",
  "duplicate",
  "pending_verification",
  "rejected",
  "resolved",
]);

interface AnalyzerEnv {
  DB: D1Database;
  EVIDENCE: R2Bucket;
  VERIFICATION_QUEUE: Queue<{ reportId: string }>;
  REPORTER_CONTACT_KEY: string;
  REPORTER_CONTACT_KEY_VERSION?: string;
}

type DatabaseRow = Record<string, any>;
type JsonObject = Record<string, unknown>;

export async function handleApi(
  request: Request,
  env: AnalyzerEnv,
  actor: string,
): Promise<Response> {
  try {
    const url = new URL(request.url);
    if (request.method === "GET" && url.pathname === "/api/summary") {
      return json(await summary(env));
    }
    if (request.method === "GET" && url.pathname === "/api/reports") {
      return json(await listReports(url, env));
    }

    const reportMatch = url.pathname.match(/^\/api\/reports\/(rpt_[a-z0-9_]+)$/);
    if (request.method === "GET" && reportMatch) {
      return await reportDetail(reportMatch[1]!, env);
    }
    const actionMatch = url.pathname.match(/^\/api\/reports\/(rpt_[a-z0-9_]+)\/actions$/);
    if (request.method === "POST" && actionMatch) {
      return await reportAction(actionMatch[1]!, request, env, actor);
    }
    const contactMatch = url.pathname.match(/^\/api\/reports\/(rpt_[a-z0-9_]+)\/contact$/);
    if (request.method === "POST" && contactMatch) {
      return await revealContact(contactMatch[1]!, env, actor);
    }
    const evidenceMatch = url.pathname.match(
      /^\/api\/reports\/(rpt_[a-z0-9_]+)\/evidence\/([^/]+)$/,
    );
    if (request.method === "GET" && evidenceMatch) {
      return await downloadEvidence(
        evidenceMatch[1]!,
        decodeURIComponent(evidenceMatch[2]!),
        env,
      );
    }
    return json({ error: { code: "not_found", message: "route not found" } }, 404);
  } catch (error) {
    if (error instanceof ApiError) {
      return json({ error: { code: error.code, message: error.message } }, error.status);
    }
    console.error("report analyzer API failed", safeError(error));
    return json(
      { error: { code: "analyzer_failed", message: "the analyzer could not complete the request" } },
      500,
    );
  }
}

async function summary(env: AnalyzerEnv) {
  const result = await env.DB.prepare(
    `SELECT status, COUNT(*) AS count FROM reports GROUP BY status`,
  ).all<{ status: string; count: number }>();
  const statuses = Object.fromEntries((result.results || []).map((row) => [row.status, row.count]));
  const total = Object.values(statuses).reduce((sum, count) => sum + Number(count), 0);
  const recurring = await env.DB.prepare(
    `SELECT COUNT(*) AS count FROM (
       SELECT fingerprint FROM reports GROUP BY fingerprint HAVING COUNT(*) > 1
     )`,
  ).first<{ count: number }>();
  return { total, statuses, recurringFingerprints: Number(recurring?.count || 0) };
}

async function listReports(url: URL, env: AnalyzerEnv) {
  const limit = Math.min(100, Math.max(1, integer(url.searchParams.get("limit"), 40)));
  const conditions: string[] = [];
  const bindings: Array<string | number> = [];
  const status = url.searchParams.get("status");
  if (status) {
    if (!REPORT_STATUSES.has(status)) throw new ApiError("invalid status filter", 400, "invalid_filter");
    conditions.push("status = ?");
    bindings.push(status);
  }
  const search = String(url.searchParams.get("search") || "").trim().slice(0, 120);
  if (search) {
    const pattern = `%${escapeLike(search)}%`;
    conditions.push(
      `(id LIKE ? ESCAPE '\\' OR operation LIKE ? ESCAPE '\\' OR title LIKE ? ESCAPE '\\' OR fingerprint LIKE ? ESCAPE '\\')`,
    );
    bindings.push(pattern, pattern, pattern, pattern);
  }
  const before = url.searchParams.get("before");
  if (before) {
    if (Number.isNaN(Date.parse(before))) throw new ApiError("invalid page cursor", 400, "invalid_cursor");
    conditions.push("created_at < ?");
    bindings.push(before);
  }
  const where = conditions.length ? `WHERE ${conditions.join(" AND ")}` : "";
  const result = await env.DB.prepare(
    `SELECT id, status, service, service_version, simile_version, artifact_digest, fingerprint,
            operation, title, kind, trigger, rejection_reason, duplicate_of, created_at, updated_at,
            verified_at, resolved_at, evidence_deleted_at, contact_deleted_at,
            COUNT(*) OVER (PARTITION BY fingerprint) AS fingerprint_count
       FROM reports ${where}
      ORDER BY created_at DESC, id DESC LIMIT ?`,
  )
    .bind(...bindings, limit + 1)
    .all<DatabaseRow>();
  const rows = result.results || [];
  const hasMore = rows.length > limit;
  const reports = rows.slice(0, limit).map(publicReport);
  return {
    reports,
    hasMore,
    nextBefore: hasMore ? reports.at(-1)?.createdAt : null,
  };
}

async function reportDetail(reportId: string, env: AnalyzerEnv): Promise<Response> {
  requireReportId(reportId);
  const [row, events] = await Promise.all([
    env.DB.prepare(
      `SELECT r.*,
              (SELECT COUNT(*) FROM reports grouped WHERE grouped.fingerprint = r.fingerprint)
                AS fingerprint_count,
              EXISTS(SELECT 1 FROM reporter_contacts c WHERE c.report_id = r.id) AS has_contact
         FROM reports r WHERE r.id = ?`,
    )
      .bind(reportId)
      .first<DatabaseRow>(),
    env.DB.prepare(
      `SELECT id, kind, actor, detail, created_at
         FROM report_events WHERE report_id = ? ORDER BY created_at ASC, id ASC`,
    )
      .bind(reportId)
      .all<DatabaseRow>(),
  ]);
  if (!row) throw new ApiError("report not found", 404, "report_not_found");

  let reportDocument = null;
  let bundle = null;
  let requestDocument = null;
  if (!row.evidence_deleted_at) {
    [reportDocument, bundle, requestDocument] = await Promise.all([
      readEvidenceJson(env.EVIDENCE, `${row.object_prefix}/report.json`),
      readEvidenceJson(env.EVIDENCE, `${row.object_prefix}/bundle.json`),
      readEvidenceJson(env.EVIDENCE, `${row.object_prefix}/request.json`),
    ]);
  }
  return json({
    report: {
      ...publicReport(row),
      hasContact: Boolean(row.has_contact),
      rejectionReason: row.rejection_reason,
      resolutionSummary: row.resolution_summary,
      reportDocument,
      bundle,
      request: requestDocument,
    },
    events: (events.results || []).map((event) => ({
      id: event.id,
      kind: event.kind,
      actor: event.actor,
      detail: parseJson(event.detail, {}),
      createdAt: event.created_at,
    })),
    evidence: row.evidence_deleted_at
      ? []
      : [...EVIDENCE_NAMES].map((name) => ({
          name,
          url: `/api/reports/${encodeURIComponent(reportId)}/evidence/${encodeURIComponent(name)}`,
        })),
  });
}

async function reportAction(
  reportId: string,
  request: Request,
  env: AnalyzerEnv,
  actor: string,
): Promise<Response> {
  requireReportId(reportId);
  requireJson(request);
  const value: unknown = await request.json();
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new ApiError("action body must be an object", 400, "invalid_action");
  }
  const input = value as JsonObject;
  const action = input.action;
  const report = await env.DB.prepare(
    `SELECT id, status, service, verification_reproduced, evidence_deleted_at FROM reports WHERE id = ?`,
  )
    .bind(reportId)
    .first<DatabaseRow>();
  if (!report) throw new ApiError("report not found", 404, "report_not_found");
  const now = new Date().toISOString();

  if (action === "reverify") {
    if (report.evidence_deleted_at) {
      throw new ApiError("report evidence has expired", 409, "evidence_expired");
    }
    await env.DB.batch([
      env.DB.prepare(
        `UPDATE reports SET status = 'pending_verification', rejection_reason = NULL,
          verification_reproduced = NULL, verification_fingerprint = NULL, verified_at = NULL,
          duplicate_of = NULL, resolved_at = NULL, resolution_summary = NULL, updated_at = ?
          WHERE id = ?`,
      ).bind(now, reportId),
      eventStatement(env, reportId, "reverification_requested", actor, {}, now),
    ]);
    await env.VERIFICATION_QUEUE.send({ reportId });
    return json({ id: reportId, status: "pending_verification" });
  }

  if (action === "resolve") {
    if (!["accepted", "rejected", "duplicate"].includes(report.status)) {
      throw new ApiError("only reviewed reports can be resolved", 409, "invalid_transition");
    }
    const summary = requiredText(input.summary, "summary", 2000);
    await env.DB.batch([
      env.DB.prepare(
        `UPDATE reports SET status = 'resolved', resolved_at = ?, resolution_summary = ?, updated_at = ?
          WHERE id = ?`,
      ).bind(now, summary, now, reportId),
      eventStatement(env, reportId, "resolved", actor, { summary }, now),
    ]);
    return json({ id: reportId, status: "resolved" });
  }

  if (action === "reject") {
    const reason = requiredText(input.reason, "reason", 2000);
    await env.DB.batch([
      env.DB.prepare(
        `UPDATE reports SET status = 'rejected', rejection_reason = ?, resolved_at = NULL,
          resolution_summary = NULL, updated_at = ? WHERE id = ?`,
      ).bind(reason, now, reportId),
      eventStatement(env, reportId, "rejected_by_reviewer", actor, { reason }, now),
    ]);
    return json({ id: reportId, status: "rejected" });
  }

  if (action === "mark_duplicate") {
    const duplicateOf = requiredText(input.duplicateOf, "duplicateOf", 80);
    requireReportId(duplicateOf);
    if (duplicateOf === reportId) {
      throw new ApiError("a report cannot duplicate itself", 400, "invalid_duplicate");
    }
    const target = await env.DB.prepare("SELECT service FROM reports WHERE id = ?")
      .bind(duplicateOf)
      .first<{ service: string }>();
    if (!target || target.service !== report.service) {
      throw new ApiError("duplicate target is not a report for this service", 409, "invalid_duplicate");
    }
    await env.DB.batch([
      env.DB.prepare(
        `UPDATE reports SET status = 'duplicate', duplicate_of = ?, resolved_at = ?, updated_at = ?
          WHERE id = ?`,
      ).bind(duplicateOf, now, now, reportId),
      eventStatement(env, reportId, "marked_duplicate", actor, { duplicateOf }, now),
    ]);
    return json({ id: reportId, status: "duplicate", duplicateOf });
  }

  throw new ApiError("unsupported report action", 400, "invalid_action");
}

async function revealContact(
  reportId: string,
  env: AnalyzerEnv,
  actor: string,
): Promise<Response> {
  requireReportId(reportId);
  const contact = await env.DB.prepare(
    `SELECT channel, ciphertext, iv, key_version FROM reporter_contacts WHERE report_id = ?`,
  )
    .bind(reportId)
    .first<DatabaseRow>();
  if (!contact) throw new ApiError("reporter contact is unavailable", 404, "contact_unavailable");
  const configuredVersion = env.REPORTER_CONTACT_KEY_VERSION || "v1";
  if (contact.key_version !== configuredVersion) {
    throw new ApiError("reporter contact uses an unavailable key version", 409, "contact_key_unavailable");
  }
  const address = await decryptContact(
    env.REPORTER_CONTACT_KEY,
    reportId,
    contact.ciphertext,
    contact.iv,
  );
  const now = new Date().toISOString();
  await eventStatement(env, reportId, "contact_revealed", actor, {}, now).run();
  return json({ channel: contact.channel, address });
}

async function downloadEvidence(
  reportId: string,
  name: string,
  env: AnalyzerEnv,
): Promise<Response> {
  requireReportId(reportId);
  if (!EVIDENCE_NAMES.has(name)) throw new ApiError("evidence file is not available", 404, "not_found");
  const report = await env.DB.prepare(
    "SELECT object_prefix, evidence_deleted_at FROM reports WHERE id = ?",
  )
    .bind(reportId)
    .first<DatabaseRow>();
  if (!report) throw new ApiError("report not found", 404, "report_not_found");
  if (report.evidence_deleted_at) throw new ApiError("report evidence has expired", 410, "evidence_expired");
  const object = await env.EVIDENCE.get(`${report.object_prefix}/${name}`);
  if (!object) throw new ApiError("evidence file is not available", 404, "not_found");
  return new Response(object.body, {
    headers: {
      "cache-control": "no-store",
      "content-disposition": `attachment; filename="${name}"`,
      "content-type": object.httpMetadata?.contentType || "application/octet-stream",
      "x-content-type-options": "nosniff",
    },
  });
}

export async function decryptContact(
  encodedKey: string,
  reportId: string,
  ciphertext: string,
  encodedIv: string,
): Promise<string> {
  if (!encodedKey) throw new Error("REPORTER_CONTACT_KEY is not configured");
  const keyBytes = decodeBase64(encodedKey);
  if (keyBytes.byteLength !== 32) throw new Error("REPORTER_CONTACT_KEY must decode to 32 bytes");
  const key = await crypto.subtle.importKey("raw", keyBytes, "AES-GCM", false, ["decrypt"]);
  const plaintext = await crypto.subtle.decrypt(
    {
      name: "AES-GCM",
      iv: decodeBase64(encodedIv),
      additionalData: new TextEncoder().encode(reportId),
    },
    key,
    decodeBase64(ciphertext),
  );
  return new TextDecoder().decode(plaintext);
}

function publicReport(row: DatabaseRow) {
  return {
    id: row.id,
    status: row.status,
    service: row.service,
    serviceVersion: row.service_version,
    simileVersion: row.simile_version,
    artifactDigest: row.artifact_digest,
    fingerprint: row.fingerprint,
    fingerprintCount: Number(row.fingerprint_count || 1),
    operation: row.operation,
    title: row.title,
    kind: row.kind,
    trigger: row.trigger,
    rejectionReason: row.rejection_reason,
    duplicateOf: row.duplicate_of,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    verifiedAt: row.verified_at,
    resolvedAt: row.resolved_at,
    evidenceDeletedAt: row.evidence_deleted_at,
    contactDeletedAt: row.contact_deleted_at,
  };
}

async function readEvidenceJson(bucket: R2Bucket, key: string): Promise<unknown> {
  const object = await bucket.get(key);
  if (!object) return null;
  try {
    return JSON.parse(await object.text());
  } catch {
    return null;
  }
}

function eventStatement(
  env: AnalyzerEnv,
  reportId: string,
  kind: string,
  actor: string,
  detail: JsonObject,
  createdAt: string,
): D1PreparedStatement {
  return env.DB.prepare(
    `INSERT INTO report_events (id, report_id, kind, actor, detail, created_at)
     VALUES (?, ?, ?, ?, ?, ?)`,
  ).bind(
    crypto.randomUUID(),
    reportId,
    kind,
    String(actor || "unknown").slice(0, 254),
    JSON.stringify(detail),
    createdAt,
  );
}

function requireReportId(value: string): void {
  if (!REPORT_ID.test(value)) throw new ApiError("invalid report identifier", 400, "invalid_report_id");
}

function requiredText(value: unknown, name: string, maximum: number): string {
  if (typeof value !== "string" || value.trim() === "") {
    throw new ApiError(`${name} is required`, 400, "invalid_action");
  }
  return value.trim().slice(0, maximum);
}

function requireJson(request: Request): void {
  if (!request.headers.get("content-type")?.startsWith("application/json")) {
    throw new ApiError("content-type must be application/json", 415, "unsupported_media_type");
  }
}

function integer(value: string | null, fallback: number): number {
  const parsed = Number.parseInt(value || String(fallback), 10);
  return Number.isSafeInteger(parsed) ? parsed : fallback;
}

function escapeLike(value: string): string {
  return value.replace(/[\\%_]/g, (character) => `\\${character}`);
}

function parseJson<T>(value: string, fallback: T): T | unknown {
  try {
    return JSON.parse(value);
  } catch {
    return fallback;
  }
}

function decodeBase64(value: string): Uint8Array {
  return Uint8Array.from(atob(value), (character) => character.charCodeAt(0));
}

function safeError(error: unknown): { name?: string; message: string } {
  return error instanceof Error ? { name: error.name, message: error.message } : { message: "unknown error" };
}

function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: {
      "cache-control": "no-store",
      "content-type": "application/json; charset=utf-8",
      "x-content-type-options": "nosniff",
    },
  });
}

class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(message: string, status: number, code: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}
