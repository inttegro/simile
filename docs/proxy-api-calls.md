---
title: Proxy API calls
description: Connect a local application to a real service through Simile.
---

Start the proxy:

```bash
simile proxy acme \
  --to https://api.acme.com \
  --learn
```

Simile prints three addresses:

```text
listen   http://127.0.0.1:8484
upstream https://api.acme.com
API base http://127.0.0.1:8484
```

Set your application's provider base URL to the printed **API base**.

Your application still uses its real provider credentials. Simile forwards them to the provider.
When learning is enabled, Simile redacts recognized credentials before it writes evidence to disk.

## Follow one request

For example, your application calls:

```text
http://127.0.0.1:8484/v1/orders?limit=10
```

Simile forwards the call to:

```text
https://api.acme.com/v1/orders?limit=10
```

Simile forwards the method, path, query, headers, and body. It returns the provider's status,
headers, and body to your application. If the provider returns a redirect, Simile returns that
redirect to your application.

In most integrations, the base URL is the only setting you need to change:

```text
ACME_BASE_URL=https://api.acme.com
```

becomes:

```text
ACME_BASE_URL=http://127.0.0.1:8484
```

Restart your application if it reads this setting only at startup. Simile logs the method, path,
status, request and response sizes, and elapsed time for each exchange.

## Exercise useful cases

Call the operations your application actually depends on. Include:

- successful requests;
- invalid input and other public errors;
- operations with more than one successful status;
- optional inputs that change the response; and
- repeated values such as IDs, references, amounts, and page numbers.

Repeated calls help Simile distinguish a stable request-to-response relationship from a value that
matched only by chance.

## Local proxy endpoints

The proxy reserves these paths:

| Path | Purpose |
| --- | --- |
| `/.simile/health` | Check whether the proxy is running. |
| `/.simile/learning` | See whether learning is active and how many exchanges Simile recorded. |

The `/.simile/` prefix is reserved for Simile. Provider paths under that prefix cannot be proxied.

## Stop the session

Keep the proxy running while you test your integration. Press Ctrl-C when you finish. If you used
`--learn`, Simile finalizes the report, writes the draft service files, and checks that the draft
compiles.

Next, [review the learned output](/proxy-learning-output).
