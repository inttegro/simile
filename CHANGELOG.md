# Changelog

All notable changes to Simile will be documented in this file.

## Unreleased

- Establish the Rust workspace, command architecture, tests, CI and release configuration.
- Execute operations directly from local or hosted OpenAPI contracts, with structural or
  `operationId` selection, schema-valid request synthesis for declared media types, and a
  response-only default output suitable for pipelines.
- Generate varied schema-valid request and response values, with seeded reproduction and
  in-memory replay for idempotency keys and GET resource identities.
