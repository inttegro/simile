import appScript from "../generated/app.client.js";
import indexHtml from "../web/index.html";
import reportStyles from "../web/styles.css";
import interFont from "../../../../web/assets/fonts/inter/InterVariable-Latin.woff2";
import simileStyles from "../../../../web/styles.css";

import { handleApi } from "./api.ts";

interface AnalyzerEnv {
  DB: D1Database;
  EVIDENCE: R2Bucket;
  VERIFICATION_QUEUE: Queue<{ reportId: string }>;
  REPORTER_CONTACT_KEY: string;
  REPORTER_CONTACT_KEY_VERSION?: string;
  LOCAL_ADMIN_BYPASS?: string;
}

const ASSET_HEADERS = {
  "cache-control": "private, no-store",
  "content-security-policy": "default-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
  "cross-origin-opener-policy": "same-origin",
  "referrer-policy": "no-referrer",
  "x-content-type-options": "nosniff",
};

export default {
  async fetch(
    request: Request,
    env: AnalyzerEnv,
    ctx: ExecutionContext,
  ): Promise<Response> {
    const actor = await authenticatedActor(request, env, ctx);
    if (!actor) {
      return new Response("Cloudflare Access authentication is required.", {
        status: 401,
        headers: { ...ASSET_HEADERS, "content-type": "text/plain; charset=utf-8" },
      });
    }
    const url = new URL(request.url);
    if (url.pathname.startsWith("/api/")) return handleApi(request, env, actor);
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("Method not allowed", { status: 405, headers: ASSET_HEADERS });
    }
    if (url.pathname === "/app.js") return asset(appScript, "text/javascript; charset=utf-8");
    if (url.pathname === "/styles.css") {
      // Why: consuming Simile Web's real design system prevents the analyzer from silently
      // developing a parallel shell, token set, or component language.
      return asset(`${simileStyles}\n${reportStyles}`, "text/css; charset=utf-8");
    }
    if (url.pathname === "/assets/fonts/inter/InterVariable-Latin.woff2") {
      return asset(interFont, "font/woff2");
    }
    if (url.pathname === "/" || url.pathname === "/index.html") {
      return asset(indexHtml, "text/html; charset=utf-8");
    }
    return new Response("Not found", { status: 404, headers: ASSET_HEADERS });
  },
};

async function authenticatedActor(
  request: Request,
  env: AnalyzerEnv,
  ctx: ExecutionContext,
): Promise<string | null> {
  const hostname = new URL(request.url).hostname;
  if (
    env.LOCAL_ADMIN_BYPASS === "true" &&
    (hostname === "127.0.0.1" || hostname === "localhost")
  ) {
    return "local-development";
  }
  if (!ctx?.access) return null;
  const identity = await ctx.access.getIdentity();
  if (identity?.email) return identity.email;
  return typeof identity?.sub === "string" ? identity.sub : "access-authenticated";
}

function asset(body: BodyInit, contentType: string): Response {
  return new Response(body, { headers: { ...ASSET_HEADERS, "content-type": contentType } });
}
