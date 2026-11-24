import { spawn } from "node:child_process";
import { createServer } from "node:http";
import type { IncomingMessage, ServerResponse } from "node:http";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { classifyRunResult, type RunResult } from "./verdict.ts";

const PORT = Number.parseInt(process.env.PORT || "8080", 10);
const MAX_BODY_BYTES = 12 * 1024 * 1024;
const REQUIRED_PARTS = ["bundle.json", "report.json", "request.json", "service.similec"];
const OPTIONAL_PARTS = ["request.body"];

interface VerifyOptions {
  simileBin?: string;
  timeoutMs?: number;
}

interface ReportDocument {
  service?: string;
}

const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url || "/", `http://${request.headers.host || "container"}`);
    if (request.method === "GET" && url.pathname === "/health") {
      return send(response, 200, { status: "ok" });
    }
    if (request.method !== "POST" || url.pathname !== "/v1/verify") {
      return send(response, 404, { error: { code: "not_found", message: "route not found" } });
    }
    const bytes = await readBody(request, MAX_BODY_BYTES);
    const webRequest = new Request("http://container/v1/verify", {
      method: "POST",
      headers: { "content-type": request.headers["content-type"] || "" },
      body: bytes,
    });
    const result = await verifyForm(await webRequest.formData());
    return send(response, 200, result);
  } catch (error) {
    console.error("container verification failed", safeError(error));
    return send(response, error instanceof InputError ? error.status : 500, {
      error: {
        code: error instanceof InputError ? "invalid_evidence" : "verification_failed",
        message: error instanceof InputError ? error.message : "verification could not run",
      },
    });
  }
});

server.listen(PORT, "0.0.0.0");

export async function verifyForm(form: FormData, options: VerifyOptions = {}) {
  const directory = await mkdtemp(join(tmpdir(), "simile-report-"));
  const bundle = join(directory, "case.simile-repro");
  try {
    await mkdir(bundle, { recursive: true });
    for (const name of [...REQUIRED_PARTS, ...OPTIONAL_PARTS]) {
      const part = form.get(name);
      if (part == null && OPTIONAL_PARTS.includes(name)) continue;
      if (!(part instanceof File)) throw new InputError(`${name} is required`);
      await writeFile(join(bundle, name), new Uint8Array(await part.arrayBuffer()));
    }
    const reportPart = form.get("report.json");
    if (!(reportPart instanceof File)) throw new InputError("report.json is required");
    const report = JSON.parse(await reportPart.text()) as ReportDocument;
    const service = report.service;
    if (!service || !/^[a-z][a-z0-9_-]{0,63}$/.test(service)) {
      throw new InputError("report service is invalid");
    }
    const output = await run(
      options.simileBin || process.env.SIMILE_BIN || "/usr/local/bin/simile",
      [
        "--output",
        "json",
        "report",
        service,
        "--bundle",
        bundle,
        "--title",
        "receiver verification",
      ],
      {
        SIMILE_HOME: join(directory, "home"),
        SIMILE_COLOR: "never",
      },
      options.timeoutMs || 30_000,
    );
    return classifyRunResult(output);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function readBody(request: IncomingMessage, limit: number): Promise<Buffer> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    size += bytes.length;
    if (size > limit) throw new InputError(`verification body exceeds ${limit} bytes`, 413);
    chunks.push(bytes);
  }
  return Buffer.concat(chunks);
}

function run(
  command: string,
  args: string[],
  extraEnv: NodeJS.ProcessEnv,
  timeoutMs: number,
): Promise<RunResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      env: { ...process.env, ...extraEnv },
      shell: false,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    let stdoutBytes = 0;
    let stderrBytes = 0;
    const maximum = 64 * 1024;
    const timer = setTimeout(() => child.kill("SIGKILL"), timeoutMs);
    child.stdout.on("data", (chunk) => {
      stdoutBytes += chunk.length;
      if (stdoutBytes <= maximum) stdout.push(chunk);
    });
    child.stderr.on("data", (chunk) => {
      stderrBytes += chunk.length;
      if (stderrBytes <= maximum) stderr.push(chunk);
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.on("close", (exitCode) => {
      clearTimeout(timer);
      resolve({
        exitCode,
        stdout: Buffer.concat(stdout).toString("utf8"),
        stderr: Buffer.concat(stderr).toString("utf8"),
      });
    });
  });
}

function send(response: ServerResponse, status: number, value: unknown): void {
  response.writeHead(status, {
    "cache-control": "no-store",
    "content-type": "application/json; charset=utf-8",
    "x-content-type-options": "nosniff",
  });
  response.end(JSON.stringify(value));
}

function safeError(error: unknown): { name?: string; message: string } {
  return error instanceof Error ? { name: error.name, message: error.message } : { message: "unknown error" };
}

class InputError extends Error {
  readonly status: number;

  constructor(message: string, status = 400) {
    super(message);
    this.name = "InputError";
    this.status = status;
  }
}
