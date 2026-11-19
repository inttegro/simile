---
title: Compatibility
description: Understand the versioned boundaries that keep artifacts, state, and reproductions trustworthy.
---

Simile persists executable artifacts and state, so compatibility is a runtime safety property—not
just a release-note concern. Each durable boundary identifies itself and rejects data it cannot
interpret safely.

| Boundary | Current version | Protection |
| --- | ---: | --- |
| `.similec` artifact | 16 | fixed envelope, length, SHA-256 digest, flags, typed indexes |
| Stack command protocol | 1 | exhaustive command/result tags and resource-kind checks |
| redb state layout | 2 | format and protocol metadata, service identity, artifact digest, resource digest |
| reproduction bundle | 1 | bundle version, Simile version, artifact format, artifact digest |

The artifact decoder rejects unknown versions, non-zero flags, truncation, trailing bytes, digest
mismatch, unknown tags, and invalid indexes before constructing a runtime. Durable service state
is also bound to both the service identity and artifact digest; a different service or incompatible
compiled definition cannot silently adopt it.

Version-one Stack snapshots migrate to the normalized redb layout in place. Other incompatible
formats fail with their found and supported versions. Simile never “best effort” decodes unknown
persistent data because a plausible but wrong service state is more dangerous than a loud startup
failure.

Reproduction bundles carry their exact artifact. The bundle manifest records compatibility
metadata for diagnostics, while the artifact envelope remains the authoritative executable format.
