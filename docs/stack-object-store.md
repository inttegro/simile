---
title: Object store
description: Local bytes, metadata, content-derived ETags, listing, and conditional writes.
---

`simile-object-store` models named immutable-or-replaceable blobs such as exports, uploads, email
attachments, and generated reports.

## Capabilities

- put and get bytes
- inspect metadata without returning bytes
- delete an object
- list metadata by key prefix in deterministic key order
- require an object to be absent or match an exact ETag before mutation

ETags are the lowercase SHA-256 digest of the stored bytes. Replacing content therefore produces a
new stable ETag; storing identical bytes produces the same ETag. Conditional writes make races and
safe retries testable without reproducing a cloud storage API.

Object metadata includes key, size, content type, logical update time, and provider-supplied string
attributes. Signed URLs, regions, buckets, lifecycle rules, and vendor authorization policies are
outside the abstraction.
