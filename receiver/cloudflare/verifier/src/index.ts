import { DurableObject } from "cloudflare:workers";

import {
  OPTIONAL_EVIDENCE,
  REQUIRED_EVIDENCE,
  VerificationError,
  parseAllowedServices,
  validateVerificationRequest,
} from "./protocol.ts";

const PORT = 8080;
const INACTIVITY_TIMEOUT_MS = 2 * 60 * 1000;
const JSON_HEADERS = {
  "cache-control": "no-store",
  "content-type": "application/json; charset=utf-8",
  "x-content-type-options": "nosniff",
};

interface VerifierEnv {
  EVIDENCE: R2Bucket;
  VERIFIER_CONTAINER: DurableObjectNamespace<VerifierContainer>;
  ALLOWED_SERVICES: string;
  VERIFIER_POOL_SIZE?: string;
}

interface ContainerResult {
  reproduced?: boolean;
  fingerprint?: string | null;
  reason?: string | null;
  error?: { code?: string };
}

export default {
  async fetch(request: Request, env: VerifierEnv): Promise<Response> {
    try {
      const url = new URL(request.url);
      if (request.method !== "POST" || url.pathname !== "/v1/verify") {
        return json({ error: { code: "not_found", message: "route not found" } }, 404);
      }
      if (!request.headers.get("content-type")?.startsWith("application/json")) {
        throw new VerificationError("content-type must be application/json", 415);
      }
      const input = validateVerificationRequest(
        await request.json(),
        parseAllowedServices(env.ALLOWED_SERVICES),
      );
      const evidence = await loadEvidence(input.objectPrefix, env.EVIDENCE);
      const form = new FormData();
      for (const [name, object] of evidence) {
        form.set(
          name,
          new File([await object.arrayBuffer()], name, {
            type: object.httpMetadata?.contentType || "application/octet-stream",
          }),
        );
      }
      const pool = poolFor(input.reportId, Number.parseInt(env.VERIFIER_POOL_SIZE || "2", 10));
      const response = await env.VERIFIER_CONTAINER.getByName(pool).fetch(
        new Request("https://container.internal/v1/verify", { method: "POST", body: form }),
      );
      const result = (await response.json()) as ContainerResult;
      if (!response.ok) {
        throw new Error(`container verifier returned HTTP ${response.status}: ${result?.error?.code || "unknown"}`);
      }
      return json({
        reproduced: result.reproduced === true,
        fingerprint: typeof result.fingerprint === "string" ? result.fingerprint : null,
        reason: typeof result.reason === "string" ? result.reason.slice(0, 2000) : null,
      });
    } catch (error) {
      if (error instanceof VerificationError) {
        return json({ error: { code: "invalid_verification_request", message: error.message } }, error.status);
      }
      console.error("report verifier failed", safeError(error));
      return json(
        { error: { code: "verifier_failed", message: "the verifier could not complete the reproduction" } },
        500,
      );
    }
  },
};

export class VerifierContainer extends DurableObject<VerifierEnv> {
  private starting: Promise<void> | undefined;

  constructor(ctx: DurableObjectState, env: VerifierEnv) {
    super(ctx, env);
    this.starting = undefined;
    const container = ctx.container;
    if (container?.running) {
      void ctx.blockConcurrencyWhile(() => container.setInactivityTimeout(INACTIVITY_TIMEOUT_MS));
    }
  }

  override async fetch(request: Request): Promise<Response> {
    this.starting ??= this.startAndWaitForPort().finally(() => {
      this.starting = undefined;
    });
    await this.starting;
    const url = new URL(request.url);
    url.protocol = "http:";
    url.host = "container";
    const forwarded = new Request(url, request);
    forwarded.headers.delete("host");
    const container = this.ctx.container;
    if (!container) throw new Error("container runtime is unavailable");
    return container.getTcpPort(PORT).fetch(forwarded);
  }

  private async startAndWaitForPort(): Promise<void> {
    const container = this.ctx.container;
    if (!container) throw new Error("container runtime is unavailable");
    if (!container.running) {
      // The verifier must never gain ambient network access or production credentials.
      const image = container.images.base;
      if (!image) throw new Error("verifier container image is unavailable");
      container.start({
        image,
        instance: "basic",
        enableInternet: false,
      });
    }
    await container.setInactivityTimeout(INACTIVITY_TIMEOUT_MS);
    const port = container.getTcpPort(PORT);
    let lastError: unknown;
    for (let attempt = 0; attempt < 100; attempt += 1) {
      try {
        const response = await port.fetch("http://container/health", {
          signal: AbortSignal.timeout(1000),
        });
        await response.body?.cancel();
        if (response.ok) return;
        lastError = new Error(`health check returned ${response.status}`);
      } catch (error) {
        lastError = error;
      }
      await scheduler.wait(200);
    }
    throw new Error("verifier container did not become ready", { cause: lastError });
  }
}

async function loadEvidence(
  prefix: string,
  bucket: R2Bucket,
): Promise<Map<string, R2ObjectBody>> {
  const required = await Promise.all(
    REQUIRED_EVIDENCE.map(async (name) => {
      const object = await bucket.get(`${prefix}/${name}`);
      if (!object) throw new VerificationError(`required evidence ${name} is missing`, 422);
      return [name, object] as const;
    }),
  );
  const optional = await Promise.all(
    OPTIONAL_EVIDENCE.map(
      async (name) => [name, await bucket.get(`${prefix}/${name}`)] as const,
    ),
  );
  const present = optional.filter(
    (entry): entry is readonly [string, R2ObjectBody] => entry[1] !== null,
  );
  return new Map([...required, ...present]);
}

function poolFor(reportId: string, configuredSize: number): string {
  const size = Number.isSafeInteger(configuredSize) && configuredSize > 0 ? configuredSize : 2;
  let hash = 2166136261;
  for (const character of reportId) {
    hash ^= character.charCodeAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return `verifier-${Math.abs(hash) % size}`;
}

function safeError(error: unknown): { name?: string; message: string } {
  return error instanceof Error ? { name: error.name, message: error.message } : { message: "unknown error" };
}

function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), { status, headers: JSON_HEADERS });
}
