# Simile report service for Cloudflare

This implementation receives reproducible Simile reports, independently verifies their attached
case, and exposes a private review console. The public receiver and private analyzer use separate
Workers so report submission never grants access to stored evidence.

## Architecture

```text
Simile CLI
    │
    ▼
public receiver Worker ──► D1 report state
    │                     R2 quarantined evidence
    ▼
verification Queue
    ▼
private verifier Worker ──► no-network Cloudflare Container ──► Simile binary
    │
    ▼
private analyzer Worker protected by Cloudflare Access
```

The analyzer uses the same lightweight HTML and CSS framework as Simile Web. Its browser client,
Worker APIs, receiver, and verifier are authored in TypeScript; the browser client is compiled to
JavaScript before deployment. It reads live D1 and R2 state rather than shipping example report
data.

## Security boundaries

- The receiver accepts only services named by `ALLOWED_SERVICES` and only artifacts recorded in
  `service_releases`.
- Technical evidence remains in a private R2 quarantine. Reporter contact is encrypted separately
  in D1 with AES-256-GCM and bound to the report identifier.
- The verifier Container has no Internet access and receives no production credentials.
- The analyzer refuses every request without a Cloudflare Access identity. A loopback-only bypass
  exists for local development and is disabled in the example configuration.
- Reporter-contact reveals and every review transition append an audit event.
- Scheduled retention removes contact and evidence independently while preserving non-sensitive
  report metadata and audit state.

## Provision

Install dependencies, typecheck every TypeScript runtime, build the analyzer client, and run the
protocol tests before provisioning Cloudflare resources:

```console
npm install
npm run check
npm test
```

Create one D1 database, one private R2 bucket, the verification Queue and its dead-letter Queue:

```console
wrangler d1 create simile-reports
wrangler r2 bucket create simile-report-evidence
wrangler queues create simile-report-verification
wrangler queues create simile-report-verification-dead
```

Copy each example configuration and replace the D1 identifier:

```console
cp wrangler.example.jsonc wrangler.jsonc
cp analyzer/wrangler.example.jsonc analyzer/wrangler.jsonc
cp verifier/wrangler.example.jsonc verifier/wrangler.jsonc
```

Apply both migrations in order:

```console
wrangler d1 migrations apply simile-reports --remote --config wrangler.jsonc
```

Generate one base64-encoded 32-byte contact key and install the same value in the receiver and
analyzer Workers. Do not place it in a Wrangler file:

```console
openssl rand -base64 32
wrangler secret put REPORTER_CONTACT_KEY --config wrangler.jsonc
wrangler secret put REPORTER_CONTACT_KEY --config analyzer/wrangler.jsonc
```

Record each published service artifact before accepting reports for it. Keep prior releases so a
report against an older published artifact can still be verified:

```sql
INSERT INTO service_releases
  (service, version, artifact_digest, command, current)
VALUES
  ('inttegro', 'SERVICE_VERSION', 'SHA256_DIGEST', 'simile update inttegro', 1);
```

Record the current Simile release used by upgrade preflight:

```sql
INSERT INTO simile_releases (version, command, current)
VALUES ('SIMILE_VERSION', 'brew upgrade simile', 1);
```

## Deploy

Deploy the private verifier first, then the analyzer, and finally the public receiver:

```console
npm run deploy:verifier
npm run deploy:analyzer
npm run deploy
```

Immediately protect the complete analyzer Worker with Cloudflare Access. The analyzer code fails
closed without an Access identity, so deployment does not create an unauthenticated review window.
The verifier has `workers_dev` disabled and is reachable only through the receiver's service
binding. The receiver intentionally uses its public `workers.dev` URL.

After deployment, publish this destination in the service Simfile:

```hcl
reporting {
  endpoint = "https://simile-report-receiver.ACCOUNT.workers.dev/v1/reports"
}
```

## Verify

Run the dependency-free unit suite and syntax checks:

```console
npm test
npm run check
```

Before publishing the endpoint, also verify:

1. unauthenticated analyzer requests return `401`;
2. receiver health returns `200` and no binding names are missing;
3. an unknown service and unknown artifact are rejected;
4. a valid report returns `202 pending_verification`;
5. the Queue invokes the verifier and the exact reproduction becomes `accepted`;
6. failed or mismatched reproduction becomes `rejected`;
7. the analyzer shows evidence, history, recurrence, and review actions; and
8. the retention schedule deletes expired R2 objects and contact independently.
