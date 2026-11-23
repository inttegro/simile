import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const webDir = resolve(scriptDir, '..');
const simileDir = resolve(webDir, '..');
const commerceDir = resolve(simileDir, '..');
const openapiDir = resolve(commerceDir, 'openapi');
const simfile = resolve(openapiDir, 'simile', 'Simfile');
const specificationPath = resolve(openapiDir, 'commerce.yml');
const moduleNames = ['principals.hcl', 'behaviors.hcl', 'errors.hcl', 'state.hcl', 'faults.hcl'];
const simileBinary = process.env.SIMILE_BIN || resolve(simileDir, 'target', 'debug', 'simile');
const requireFromOpenApi = createRequire(resolve(openapiDir, 'package.json'));
const YAML = requireFromOpenApi('yaml');
const methods = new Set(['delete', 'get', 'patch', 'post', 'put']);
const specification = YAML.parse(readFileSync(specificationPath, 'utf8'));
const temporaryDirectory = mkdtempSync(resolve(tmpdir(), 'simile-web-inttegro-'));
const artifactPath = resolve(temporaryDirectory, 'inttegro.similec');

try {
  const described = runJson(['--output', 'json', '-v', 'describe', 'service', '--config', simfile]);
  const compiled = runJson([
    'compile',
    specificationPath,
    ...moduleNames
      .flatMap((module) => ['--module', resolve(openapiDir, 'simile', module)]),
    '--artifact',
    artifactPath,
    '--output',
    'json',
  ]);
  const inspected = runJson(['inspect', artifactPath, '--output', 'json']);
  const audited = runJson(['audit', artifactPath, '--output', 'json']);
  const describedById = new Map(described.operations.map((operation) => [operation.operationId, operation]));
  const outcomesByKey = new Map(inspected.operationOutcomes.map((entry) => [entry.operation, entry]));
  const operations = [];

  for (const [path, pathItem] of Object.entries(specification.paths ?? {})) {
    for (const [method, operation] of Object.entries(pathItem ?? {})) {
      if (!methods.has(method) || !operation?.operationId) continue;
      const key = `${method.toUpperCase()} ${path}`;
      const description = describedById.get(operation.operationId);
      const inspectedOutcomes = outcomesByKey.get(key);
      const requestMedia = preferredMedia(operation.requestBody?.content);
      const responseEntry = preferredResponse(operation.responses);
      const responseMedia = preferredMedia(responseEntry?.response?.content);
      const parameterExample = Object.fromEntries(
        [...(pathItem.parameters ?? []), ...(operation.parameters ?? [])]
          .map(resolveReference)
          .filter(Boolean)
          .map((parameter) => [parameter.name, sampleParameter(parameter)]),
      );
      const requestBodyExample = sampleMedia(requestMedia);
      const requestExample = isObject(requestBodyExample)
        ? { ...parameterExample, ...requestBodyExample }
        : Object.keys(parameterExample).length > 0
          ? parameterExample
          : requestBodyExample;
      const rawOutcomes = inspectedOutcomes?.outcomes ?? description?.outcomes?.map((outcome) => ({
        ...outcome,
        provenance: outcome.name.startsWith('http_') ? 'schema_synthesized' : 'provider_authored',
      })) ?? [];
      const outcomes = rawOutcomes.map((outcome) => describeOutcome(operation, outcome));

      operations.push({
        id: operation.operationId,
        key,
        method: method.toUpperCase(),
        path,
        summary: operation.summary || description?.description || operation.operationId,
        description: operation.description || '',
        tags: operation.tags ?? [],
        secured: description?.secured ?? Boolean(operation.security ?? specification.security),
        behaviorSource: description?.behaviorSource ?? 'contract_derived',
        success: outcomes.find((outcome) => outcome.status >= 200 && outcome.status < 400)?.status ?? responseEntry?.status ?? 200,
        outcomes,
        underspecified: inspectedOutcomes?.underspecified ?? false,
        requestSchema: schemaLabel(requestMedia?.schema, 'No request body'),
        responseSchema: schemaLabel(responseMedia?.schema, responseEntry ? `${responseEntry.status} response` : 'No response body'),
        requestExample: requestExample ?? {},
        responseExample: sampleMedia(responseMedia) ?? {},
      });
    }
  }

  operations.sort((left, right) => left.tags[0]?.localeCompare(right.tags[0] ?? '') || left.path.localeCompare(right.path) || left.method.localeCompare(right.method));

  const provenanceCounts = inspected.operationOutcomes
    .flatMap((operation) => operation.outcomes)
    .reduce((counts, outcome) => {
      counts[outcome.provenance] = (counts[outcome.provenance] ?? 0) + 1;
      return counts;
    }, {});
  const domainCounts = operations.reduce((counts, operation) => {
    const domain = operation.tags[0] || 'Other';
    counts[domain] = (counts[domain] ?? 0) + 1;
    return counts;
  }, {});
  const statusCounts = operations
    .flatMap((operation) => operation.outcomes)
    .reduce((counts, outcome) => {
      const group = `${Math.floor(outcome.status / 100)}xx`;
      counts[group] = (counts[group] ?? 0) + 1;
      return counts;
    }, {});
  const findingCounts = audited.findings.reduce((counts, finding) => {
    counts[finding.code] = (counts[finding.code] ?? 0) + 1;
    return counts;
  }, {});

  const model = {
    service: {
      name: described.service,
      description: described.description,
      version: described.serviceVersion,
      openapiVersion: described.openapiVersion,
      listen: described.listen,
      source: 'openapi/simile/Simfile',
      contract: 'openapi/commerce.yml',
      reportingEndpoint: described.reportingEndpoint,
    },
    configuration: {
      files: [
        sourceFile('simfile', 'Simfile', simfile),
        ...moduleNames.map((name) => sourceFile(name, name, resolve(openapiDir, 'simile', name))),
      ],
    },
    artifact: {
      compilerVersion: inspected.compilerVersion,
      formatVersion: inspected.formatVersion,
      bytes: compiled.artifact.bytes,
      sha256: inspected.payloadSha256,
      publicExportSafe: inspected.publicExportSafe,
    },
    metrics: {
      operations: inspected.operations,
      responses: inspected.responses,
      outcomes: inspected.outcomes,
      providerAuthoredOutcomes: inspected.providerAuthoredOutcomes,
      schemaSynthesizedOutcomes: inspected.schemaSynthesizedOutcomes,
      underspecifiedOperations: inspected.underspecifiedOperations,
      scenarioSelections: inspected.scenarioSelections,
      scenarios: described.scenarios.length,
      faults: inspected.faults,
      faultProfiles: inspected.faultProfiles,
      backends: described.backends.length,
      asyncOperations: described.asyncOperations.length,
      principals: described.principals.length,
      profiles: described.profiles.length,
      errors: audited.errors,
      warnings: audited.warnings,
      provenanceCounts,
      domainCounts,
      statusCounts,
      findingCounts,
    },
    operations,
    scenarios: described.scenarios,
    faults: described.faults,
    faultProfiles: described.faultProfiles,
    backends: described.backends,
    asyncOperations: described.asyncOperations,
    protocolErrors: described.protocolErrors,
    principals: described.principals,
    profiles: described.profiles,
    principalSetups: described.principalSetups,
    findings: audited.findings,
  };

  const output = `// Generated by scripts/build-inttegro-service.mjs. Do not edit by hand.\nwindow.INTTEGRO_SERVICE_DATA = Object.freeze(${JSON.stringify(model, null, 2)});\n`;
  writeFileSync(resolve(webDir, 'inttegro-service-data.js'), output);
  console.log(`Rendered ${operations.length} Inttegro operations from ${described.serviceVersion}`);
} finally {
  rmSync(temporaryDirectory, { force: true, recursive: true });
}

function runJson(arguments_) {
  return JSON.parse(execFileSync(simileBinary, arguments_, { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }));
}

function sourceFile(id, name, path) {
  return {
    id,
    name,
    path: path.replace(`${commerceDir}/`, ''),
    language: 'hcl',
    contents: readFileSync(path, 'utf8'),
  };
}

function preferredResponse(responses = {}) {
  const entries = Object.entries(responses ?? {});
  const [status, response] = entries.find(([candidate]) => /^2\d\d$/.test(candidate))
    ?? entries.find(([candidate]) => /^3\d\d$/.test(candidate))
    ?? entries[0]
    ?? [];
  return response ? { status: Number(status) || status, response: resolveReference(response) } : undefined;
}

function preferredMedia(content = {}) {
  return preferredMediaEntry(content)?.media;
}

function preferredMediaEntry(content = {}) {
  const entries = Object.entries(content ?? {});
  const [contentType, media] = entries.find(([candidate]) => candidate === 'application/json')
    ?? entries.find(([candidate]) => candidate === 'application/problem+json')
    ?? entries[0]
    ?? [];
  return media ? { contentType, media } : undefined;
}

function describeOutcome(operation, outcome) {
  const response = resolveReference(operation.responses?.[String(outcome.status)] ?? operation.responses?.default);
  const mediaEntry = preferredMediaEntry(response?.content);
  return {
    ...outcome,
    description: response?.description ?? '',
    responseSchema: schemaLabel(mediaEntry?.media?.schema, `${outcome.status} response`),
    responseVariants: schemaVariantCount(mediaEntry?.media?.schema),
    contentType: mediaEntry?.contentType ?? 'No response body',
    headers: Object.keys(response?.headers ?? {}),
    example: sampleMedia(mediaEntry?.media) ?? null,
  };
}

function sampleMedia(media) {
  if (!media) return undefined;
  if (media.example !== undefined) return resolveExample(media.example);
  const example = Object.values(media.examples ?? {})[0];
  if (example !== undefined) return resolveExample(example);
  return sampleSchema(media.schema);
}

function resolveExample(example) {
  const resolved = resolveReference(example);
  return resolved?.value ?? resolved;
}

function schemaLabel(schema, fallback) {
  if (!schema) return fallback;
  if (schema.$ref) return schema.$ref.split('/').at(-1);
  const inheritedReference = schema.allOf?.find((branch) => branch.$ref)?.$ref;
  if (inheritedReference) return inheritedReference.split('/').at(-1);
  const resolved = resolveReference(schema);
  return resolved?.title || fallback;
}

function schemaVariantCount(schema) {
  if (!schema) return 0;
  if (schema.oneOf?.length) return schema.oneOf.length;
  if (schema.anyOf?.length) return schema.anyOf.length;
  if (schema.allOf?.length) return Math.max(1, ...schema.allOf.map(schemaVariantCount));
  return 1;
}

function sampleParameter(parameter) {
  if (parameter.example !== undefined) return parameter.example;
  return sampleSchema(parameter.schema) ?? `example_${parameter.name}`;
}

function sampleSchema(input, depth = 0, references = new Set()) {
  if (!input || depth > 5) return undefined;
  if (input.$ref) {
    if (references.has(input.$ref)) return undefined;
    const nextReferences = new Set(references).add(input.$ref);
    return sampleSchema(resolveReference(input), depth + 1, nextReferences);
  }
  if (input.example !== undefined) return input.example;
  if (input.default !== undefined) return input.default;
  if (input.const !== undefined) return input.const;
  if (input.enum?.length) return input.enum[0];
  if (input.allOf?.length) {
    return input.allOf
      .map((branch) => sampleSchema(branch, depth + 1, references))
      .filter(isObject)
      .reduce((value, branch) => ({ ...value, ...branch }), {});
  }
  const alternative = input.oneOf?.[0] ?? input.anyOf?.[0];
  if (alternative) return sampleSchema(alternative, depth + 1, references);
  if (input.type === 'array') {
    const item = sampleSchema(input.items, depth + 1, references);
    return item === undefined ? [] : [item];
  }
  if (input.type === 'object' || input.properties || input.additionalProperties) {
    const properties = Object.entries(input.properties ?? {});
    const required = new Set(input.required ?? []);
    const selected = required.size > 0 ? properties.filter(([name]) => required.has(name)) : properties.slice(0, 4);
    return Object.fromEntries(selected.map(([name, schema]) => [name, sampleSchema(schema, depth + 1, references)]));
  }
  if (input.type === 'integer' || input.type === 'number') return input.minimum ?? 1;
  if (input.type === 'boolean') return true;
  if (input.type === 'string') {
    if (input.format === 'date-time') return '2026-11-23T10:00:00Z';
    if (input.format === 'date') return '2026-11-23';
    if (input.format === 'email') return 'developer@example.test';
    if (input.format === 'uri' || input.format === 'url') return 'https://example.test';
    if (input.format === 'uuid') return '00000000-0000-4000-8000-000000000000';
    return 'string';
  }
  return undefined;
}

function resolveReference(input) {
  if (!input?.$ref?.startsWith('#/')) return input;
  return input.$ref
    .slice(2)
    .split('/')
    .map((part) => part.replaceAll('~1', '/').replaceAll('~0', '~'))
    .reduce((value, part) => value?.[part], specification);
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}
