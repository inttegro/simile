# Simile report receiver for Cloudflare

This reference receiver implements the protocol documented at
`docs/reporting-endpoint.md`. It stores technical evidence in R2, encrypted reporter contact in D1,
and sends quarantined report IDs to a Queue. A private verifier service binding must reproduce the
case before the report becomes `accepted`.

## Configure

1. Copy `wrangler.example.jsonc` to `wrangler.jsonc` and fill the D1 and service binding IDs.
2. Apply `migrations/0001_reporting.sql` to the D1 database.
3. Create the evidence bucket and verification queues named by the config.
4. Set `REPORTER_CONTACT_KEY` to a base64-encoded 32-byte AES key with `wrangler secret put`.
5. Bind `VERIFIER` to an isolated service that reads the quarantined objects and returns
   `{ "reproduced": true, "fingerprint": "…" }` only after running the exact case.

The receiver acknowledges new submissions with `pending_verification`. Missing or unavailable
verifiers cause Queue retries and never promote evidence into an accepted issue.

Run the dependency-free protocol tests with `npm test`.
