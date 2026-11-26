const serviceData = window.SIMILE_SERVICE_DATA ?? window.INTTEGRO_SERVICE_DATA;
const registeredServices = window.SIMILE_CONSOLE_SERVICES ?? [];

if (!serviceData) {
  throw new Error('Simile console service data is missing.');
}

const localServiceUrl = window.SIMILE_CONSOLE_RUNTIME ?? `http://${serviceData.service.listen}`;
const consoleToken = window.SIMILE_CONSOLE_TOKEN ?? null;
const serviceName = serviceData.service.name;
const serviceSlug = serviceName.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '') || 'service';
const activeProfiles = serviceData.profiles.filter((profile) => profile.status === 'active');
const activeProfileName = activeProfiles[0]?.name
  ?? serviceData.profiles[0]?.name
  ?? 'No active profile';
const configurationFiles = serviceData.configuration?.files ?? [];

const exchangeId = 'ex_01JDC6Y8P7Y4K0FJ9Q3T5M2N6B';

const navItems = [
  ['service', 'Service'],
  ['operations', 'Operations'],
  ['exchanges', 'Exchanges'],
  ['state', 'State'],
  ['deferred', 'Deferred work'],
  ['scenarios', 'Scenarios'],
  ['faults', 'Faults'],
  ['fidelity', 'Fidelity'],
  ['sessions', 'Sessions'],
  ['settings', 'Settings'],
];

const operationRows = serviceData.operations.map((operation) => [
  operation.method,
  operation.path,
  operation.id,
  String(operation.success),
  String(operation.outcomes.length),
  operation.behaviorSource === 'provider_authored' ? 'Provider authored' : 'Contract derived',
]);

const operationCatalog = Object.fromEntries(serviceData.operations.map((operation) => [operation.id, {
  summary: operation.summary,
  description: operation.description,
  tags: operation.tags,
  secured: operation.secured,
  underspecified: operation.underspecified,
  requestSchema: operation.requestSchema,
  responseSchema: operation.responseSchema,
  request: JSON.stringify(operation.requestExample ?? {}, null, 2),
  response: JSON.stringify(operation.responseExample ?? {}, null, 2),
  outcomes: operation.outcomes.map((outcome) => ({
    ...outcome,
    status: String(outcome.status),
    source: provenanceLabel(outcome.provenance),
    example: outcome.example === null ? null : JSON.stringify(outcome.example, null, 2),
  })),
} ]));
const operationIdByKey = new Map(serviceData.operations.map((operation) => [operation.key, operation.id]));

function provenanceLabel(provenance) {
  return {
    open_api_derived: 'OpenAPI example',
    provider_authored: 'Provider authored',
    schema_synthesized: 'Schema synthesized',
  }[provenance] || provenance;
}

const requestFormats = {
  curl: { label: 'Copy cURL', toast: 'cURL command' },
  sdk: { label: 'Copy SDK', toast: 'SDK request' },
  insomnia: { label: 'Copy Insomnia', toast: 'Insomnia request' },
  postman: { label: 'Copy Postman', toast: 'Postman collection' },
};

const representativeOperation = serviceData.operations.find((operation) => operation.id === 'createOrder')
  ?? serviceData.operations[0]
  ?? { id: 'operation', key: 'NO OPERATIONS', method: 'GET', path: '/' };
const representativeOperationDetail = operationCatalog[representativeOperation.id] ?? {
  request: '{}',
  response: '{}',
  requestSchema: 'No request body',
  responseSchema: 'No response body',
};
const representativeRequest = representativeOperationDetail.request ?? '{}';
const representativeResponse = representativeOperationDetail.response ?? '{}';
const representativeResponseSize = `${representativeResponse.length} bytes`;
const representativeOutcome = representativeOperation.outcomes?.find((outcome) => outcome.status === representativeOperation.success)
  ?? representativeOperation.outcomes?.[0];
const representativeOutcomeName = representativeOutcome?.name ?? 'success';

const exchangeRows = [
  [exchangeId, 'POST', '/orders/create', 'createOrder', '200', '38ms', 'Delivered', '19:18:04'],
  ['ex_01JDC6T8E1QM8H4F72XW1C9D3P', 'POST', '/products/page', 'pageProducts', '200', '12ms', 'Delivered', '19:17:42'],
  ['ex_01JDC6M5R9AP2F7Z1K8N4T6Q0H', 'POST', '/orders/lookup', 'lookupOrder', '200', '8ms', 'Delivered', '19:16:58'],
  ['ex_01JDC6E4B2TV9M7C3R8K5H1Q6A', 'POST', '/refunds/create', 'createRefund', '422', '15ms', 'Delivered', '19:15:21'],
  ['ex_01JDC68W7J3Q5V9N2K4A1C6F8M', 'POST', '/orders/update', 'updateOrder', '200', '21ms', 'Delivered', '19:14:47'],
  ['ex_01JDC61H8M5K3T7P9Q2R4V6N0B', 'POST', '/products/create', 'createProduct', '409', '17ms', 'Delivered', '19:13:09'],
];

const resourceGroups = {
  orders: [
    { id: 'or_simile_requires_payment', status: 'requires_payment', summary: 'GHS 200.00', kind: 'ORDER', updated: 'Nov 22, 2026, 7:18:04 PM', createdBy: 'createOrder', body: representativeResponse },
    { id: 'or_7tm2d18pz', status: 'paid', summary: 'GHS 580.00', kind: 'ORDER', updated: 'Nov 22, 2026, 6:52:14 PM', createdBy: 'createOrder', body: '{\n  "order": {\n    "id": "or_7tm2d18pz",\n    "number": "SIMILE-ORDER-002",\n    "status": "paid",\n    "total": { "currency": "ghs", "value": 58000 },\n    "paid_at": "2026-11-22T18:52:14Z"\n  }\n}' },
    { id: 'or_4wq1n68kf', status: 'cancelled', summary: 'USD 49.00', kind: 'ORDER', updated: 'Nov 22, 2026, 6:40:02 PM', createdBy: 'createOrder', body: '{\n  "order": {\n    "id": "or_4wq1n68kf",\n    "number": "SIMILE-ORDER-003",\n    "status": "cancelled",\n    "total": { "currency": "usd", "value": 4900 }\n  }\n}' },
  ],
  products: [
    { id: 'prod_01JDBZK4', status: 'active', summary: 'GHS 200.00', kind: 'PRODUCT', updated: 'Nov 22, 2026, 6:34:17 PM', createdBy: 'createProduct', body: '{\n  "product": {\n    "id": "prod_01JDBZK4",\n    "name": "Premium Cotton T-Shirt",\n    "status": "active",\n    "price": { "currency": "ghs", "value": 20000 }\n  }\n}' },
    { id: 'prod_01JDBYH7', status: 'draft', summary: 'GHS 85.00', kind: 'PRODUCT', updated: 'Nov 22, 2026, 5:59:41 PM', createdBy: 'createProduct', body: '{\n  "product": {\n    "id": "prod_01JDBYH7",\n    "name": "Canvas Tote",\n    "status": "draft",\n    "price": { "currency": "ghs", "value": 8500 }\n  }\n}' },
  ],
  buy_links: [
    { id: 'bl_01JDC7A4', status: 'active', summary: 'Product offer', kind: 'BUY LINK', updated: 'Nov 22, 2026, 6:22:07 PM', createdBy: 'createBuyLink', body: '{\n  "buy_link": {\n    "id": "bl_01JDC7A4",\n    "target": { "type": "product", "id": "prod_01JDBZK4" },\n    "url": "https://buy.inttegro.test/bl_01JDC7A4",\n    "active": true\n  }\n}' },
  ],
  customers: [
    { id: 'cu_27j469v3g', status: 'active', summary: 'Ama Mensah', kind: 'CUSTOMER', updated: 'Nov 22, 2026, 7:18:04 PM', createdBy: 'createOrder', body: '{\n  "customer": {\n    "id": "cu_27j469v3g",\n    "name": "Ama Mensah",\n    "email_address": "ama@example.test",\n    "guest": false\n  }\n}' },
  ],
};

const stackComponents = Object.fromEntries(serviceData.backends.map((backend) => {
  const [kind] = backend.reference.split('.');
  const workers = serviceData.asyncOperations.filter((operation) => operation.backends.includes(backend.reference));
  return [backend.reference, {
    label: backend.reference,
    process: kind === 'db' ? 'Resource state' : kind === 'queue' ? 'Deferred queue' : 'Scheduler',
    icon: kind === 'db' ? 'state' : 'deferred',
    status: 'DECLARED',
    activity: workers.length ? `${workers.length} async operation${workers.length === 1 ? '' : 's'}` : 'Public resource snapshots',
    description: backend.description,
    workers,
  }];
}));

const scenarioCatalog = Object.fromEntries(serviceData.scenarios.map((scenario) => [scenario.name, {
  name: scenario.name.replaceAll('_', ' ').replace(/\b\w/g, (letter) => letter.toUpperCase()),
  selections: scenario.selections,
  faults: scenario.faults,
  description: scenario.description,
}]));

const state = {
  serviceTab: 'overview',
  selectedStackComponent: serviceData.backends[0]?.reference,
  operationTab: 'contract',
  selectedOutcome: null,
  requestCopyFormat: 'curl',
  exchangeTab: 'timeline',
  exchangeFilter: 'all',
  exchangeQuery: '',
  selectedEvent: 'response',
  autoRefresh: true,
  logDock: window.localStorage.getItem('simile-log-dock') || 'collapsed',
  logQuery: '',
  logFollowTail: true,
  logScrollTop: 0,
  soundEnabled: window.localStorage.getItem('simile-sound-alerts') !== 'muted',
  eventFilter: 'all',
  selectedResource: 'or_simile_requires_payment',
  selectedResourceGroup: 'orders',
  selectedScenario: serviceData.scenarios[0]?.name,
  disabledScenarios: new Set(),
  selectedFaultProfile: serviceData.faultProfiles[0]?.name,
  selectedConfigFile: configurationFiles[0]?.id,
  generatedExchangeReport: null,
  timelineOrder: 'ascending',
  environment: 'local',
  workbenchDrafts: {},
  workbenchResult: null,
  replayResults: {},
  sidebarCollapsed: false,
  theme: document.documentElement.dataset.theme === 'light' ? 'light' : 'dark',
};
if (!['collapsed', 'partial', 'full'].includes(state.logDock)) state.logDock = 'collapsed';

const runtime = {
  connected: false,
  status: null,
  exchanges: [],
  inspectionState: null,
  deferred: null,
  error: null,
  signature: '',
  observationsReady: false,
  control: null,
  sessions: [],
  controlEvents: [],
  controlCursor: 0,
};

function runtimeExchangeRows() {
  return runtime.exchanges.map((exchange) => [
    exchange.id,
    exchange.method,
    exchange.path,
    operationIdByKey.get(exchange.operation) || exchange.operation,
    String(exchange.status),
    formatDuration(exchange.durationMicros),
    Number(exchange.status) < 400 ? 'Delivered' : 'Issue',
    new Date(exchange.startedAtMs).toLocaleTimeString([], { hour12: false }),
  ]);
}

function displayedExchangeRows() {
  return runtime.connected ? runtimeExchangeRows() : exchangeRows;
}

function formatDuration(micros) {
  if (micros < 1000) return `${micros}µs`;
  if (micros < 1_000_000) return `${(micros / 1000).toFixed(micros < 10_000 ? 1 : 0)}ms`;
  return `${(micros / 1_000_000).toFixed(2)}s`;
}

function messageBody(message) {
  if (!message || message.body == null) return message?.truncated ? 'Body omitted because it exceeds the inspection limit.' : '';
  return typeof message.body === 'string' ? message.body : JSON.stringify(message.body, null, 2);
}

function isSensitiveName(name) {
  const normalized = String(name).toLowerCase().replaceAll('-', '_').replaceAll('.', '_');
  return [
    'authorization',
    'cookie',
    'password',
    'secret',
    'token',
    'api_key',
    'apikey',
    'access_key',
    'private_key',
    'card_number',
    'account_number',
    'cvc',
    'cvv',
  ].some((sensitive) => normalized === sensitive || normalized.endsWith(`_${sensitive}`));
}

function sanitizeReportValue(value) {
  if (Array.isArray(value)) return value.map(sanitizeReportValue);
  if (!value || typeof value !== 'object') return value;
  return Object.fromEntries(Object.entries(value).map(([name, item]) => [
    name,
    isSensitiveName(name) ? '<redacted>' : sanitizeReportValue(item),
  ]));
}

function previewExchange() {
  return {
    id: route().detail || exchangeId,
    operation: representativeOperation.key,
    method: representativeOperation.method,
    path: representativeOperation.path,
    startedAtMs: Date.parse('2026-11-22T19:18:04.114Z'),
    durationMicros: 38_000,
    status: 200,
    outcome: 'requires_payment',
    profile: activeProfileName,
    request: {
      contentType: 'application/json',
      body: JSON.parse(representativeRequest),
      truncated: false,
    },
    response: {
      contentType: 'application/json',
      body: JSON.parse(representativeResponse),
      truncated: false,
    },
  };
}

function currentExchange() {
  return runtime.exchanges.find((exchange) => exchange.id === route().detail) ?? previewExchange();
}

function relatedJournalEvents(exchange) {
  if (!runtime.connected || !runtime.inspectionState?.events) return [];
  const endedAtMs = exchange.startedAtMs + exchange.durationMicros / 1000;
  return runtime.inspectionState.events
    .filter((event) => event.at >= exchange.startedAtMs && event.at <= endedAtMs)
    .map(sanitizeReportValue);
}

function exchangeReport(exchange) {
  const operationId = operationIdByKey.get(exchange.operation) || exchange.operation;
  const operation = serviceData.operations.find((candidate) => candidate.id === operationId);
  const statusCandidates = operation?.outcomes.filter((outcome) => Number(outcome.status) === Number(exchange.status)) ?? [];
  const selectedOutcome = exchange.outcome
    ? operation?.outcomes.find((outcome) => outcome.name === exchange.outcome)
    : statusCandidates.length === 1 ? statusCandidates[0] : null;
  const endedAtMs = exchange.startedAtMs + exchange.durationMicros / 1000;
  const live = runtime.exchanges.some((candidate) => candidate.id === exchange.id);

  return {
    format: 'simile.exchange-report',
    formatVersion: 1,
    generatedAt: new Date().toISOString(),
    title: `${exchange.method} ${exchange.path} returned ${exchange.status}`,
    source: live ? 'live_inspection' : 'representative_preview',
    service: {
      name: serviceData.service.name,
      version: serviceData.service.version,
      contract: serviceData.service.contract,
      openapiVersion: serviceData.service.openapiVersion,
      artifact: {
        sha256: serviceData.artifact.sha256,
        compilerVersion: serviceData.artifact.compilerVersion,
        formatVersion: serviceData.artifact.formatVersion,
      },
    },
    exchange: {
      id: exchange.id,
      operation: {
        id: operationId,
        key: exchange.operation,
        method: exchange.method,
        path: exchange.path,
      },
      profile: exchange.profile ?? null,
      outcome: exchange.outcome ?? null,
      status: Number(exchange.status),
      startedAt: new Date(exchange.startedAtMs).toISOString(),
      endedAt: new Date(endedAtMs).toISOString(),
      durationMicros: exchange.durationMicros,
      request: sanitizeReportValue(exchange.request),
      response: sanitizeReportValue(exchange.response),
    },
    contract: operation ? {
      summary: operation.summary,
      requestSchema: operation.requestSchema,
      responseSchema: operation.responseSchema,
      selectedOutcome: selectedOutcome ? {
        name: selectedOutcome.name,
        status: Number(selectedOutcome.status),
        provenance: selectedOutcome.provenance,
        description: selectedOutcome.description,
      } : null,
      statusCandidates: selectedOutcome ? [] : statusCandidates.map((outcome) => ({
        name: outcome.name,
        status: Number(outcome.status),
        provenance: outcome.provenance,
      })),
    } : null,
    relatedJournalEvents: relatedJournalEvents(exchange),
    evidenceBoundary: {
      sanitization: 'Recognized secret fields are redacted. Opaque and oversized bodies remain omitted by runtime inspection.',
      verification: 'This exchange report has not been reproduced or verified as a Simile issue report.',
      submission: 'Generated locally. Nothing was submitted to the service reporting endpoint.',
      nextStep: 'Create and verify a reproduction bundle with `simile report` before submitting an emulator inconsistency.',
    },
  };
}

function downloadFile(filename, contents, type) {
  const url = URL.createObjectURL(new Blob([contents], { type }));
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  document.body.append(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

function downloadJson(filename, value) {
  downloadFile(filename, `${JSON.stringify(value, null, 2)}\n`, 'application/json');
}

async function inspectorJson(path, options = {}) {
  const response = await fetch(`${localServiceUrl}${path}`, {
    cache: 'no-store',
    ...options,
    headers: {
      ...(options.body ? { 'Content-Type': 'application/json' } : {}),
      ...(consoleToken ? { Authorization: `Bearer ${consoleToken}` } : {}),
      ...options.headers,
    },
  });
  const body = await response.json().catch(() => null);
  if (!response.ok) {
    throw new Error(body?.error?.message || body?.message || `Inspector returned ${response.status}`);
  }
  return body;
}

async function generateExchangeReport() {
  const exchange = currentExchange();
  const report = exchangeReport(exchange);
  if (!runtime.exchanges.some((candidate) => candidate.id === exchange.id)) {
    state.generatedExchangeReport = report;
    navigate(`/exchanges/${exchange.id}/report`);
    showToast('Preview reports cannot be verified by the runtime', 'warning');
    return;
  }
  showToast('Reproducing and verifying the captured exchange…');
  try {
    const runtimeReport = await inspectorJson(`/.simile/exchanges/${encodeURIComponent(exchange.id)}/report`, {
      method: 'POST',
      body: JSON.stringify({ title: report.title }),
    });
    report.runtimeReport = runtimeReport;
    report.evidenceBoundary.verification = `Verified by Simile at ${runtimeReport.checkedAt}.`;
    report.evidenceBoundary.submission = runtimeReport.submitted
      ? 'Submitted to the configured service reporting endpoint.'
      : 'Verified locally. Nothing has been submitted.';
    report.evidenceBoundary.nextStep = runtimeReport.submitted
      ? 'Track the submitted report by its report ID.'
      : 'Review the evidence, then submit it explicitly.';
    state.generatedExchangeReport = report;
    navigate(`/exchanges/${exchange.id}/report`);
    showToast(`Verified report ${runtimeReport.id}`, 'success');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

function preparedExchangeReport(exchange) {
  const source = runtime.exchanges.some((candidate) => candidate.id === exchange.id)
    ? 'live_inspection'
    : 'representative_preview';
  if (state.generatedExchangeReport?.exchange.id === exchange.id && state.generatedExchangeReport.source === source) {
    return state.generatedExchangeReport;
  }
  state.generatedExchangeReport = exchangeReport(exchange);
  return state.generatedExchangeReport;
}

function downloadExchangeReport() {
  const exchange = currentExchange();
  const report = preparedExchangeReport(exchange);
  downloadFile(
    `simile-exchange-report-${exchange.id}.json`,
    `${JSON.stringify(report, null, 2)}\n`,
    'application/json',
  );
  document.querySelector('[data-popover]')?.setAttribute('hidden', '');
  document.querySelectorAll('[data-menu][aria-expanded="true"]').forEach((button) => button.setAttribute('aria-expanded', 'false'));
  showToast(`Generated report for ${exchange.id}`, 'success');
}

async function submitExchangeReport() {
  const report = preparedExchangeReport(currentExchange());
  if (!report.runtimeReport?.verified) {
    showToast('Verify this exchange before submitting it', 'warning');
    return;
  }
  try {
    const runtimeReport = await inspectorJson(`/.simile/reports/${encodeURIComponent(report.runtimeReport.id)}/submit`, {
      method: 'POST',
      body: JSON.stringify({ currentVersion: true }),
    });
    report.runtimeReport = runtimeReport;
    report.evidenceBoundary.submission = runtimeReport.submitted
      ? 'Submitted to the configured service reporting endpoint.'
      : 'Submission is waiting for the required upgrade.';
    state.generatedExchangeReport = report;
    renderShell();
    showToast(runtimeReport.submitted ? `Submitted report ${runtimeReport.id}` : 'Report requires an upgrade before submission', runtimeReport.submitted ? 'success' : 'warning');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

async function clearExchangeHistory() {
  if (!runtime.connected || !window.confirm('Permanently clear every captured exchange for this service?')) return;
  try {
    const result = await inspectorJson('/.simile/exchanges', { method: 'DELETE' });
    state.generatedExchangeReport = null;
    await refreshRuntime(true);
    showToast(`Cleared ${result.removed} exchange${result.removed === 1 ? '' : 's'}`, 'success');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

async function resetRuntimeState() {
  if (!runtime.connected || !window.confirm('Reset all local service resources and journal history? This cannot be undone.')) return;
  try {
    await inspectorJson('/.simile/state/reset', {
      method: 'POST',
      body: JSON.stringify({ confirm: true }),
    });
    await refreshRuntime(true);
    showToast('Local service state was reset', 'success');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

function downloadContract() {
  downloadJson(`${serviceSlug}-simile-contract.json`, {
    service: serviceData.service,
    artifact: serviceData.artifact,
    operations: serviceData.operations,
    scenarios: serviceData.scenarios,
    faults: serviceData.faults,
    faultProfiles: serviceData.faultProfiles,
    backends: serviceData.backends,
  });
  showToast('Exported the compiled contract view', 'success');
}

function downloadExchanges() {
  downloadJson('simile-exchanges.json', { exchanges: runtime.connected ? runtime.exchanges : [] });
  showToast(`Exported ${runtime.connected ? runtime.exchanges.length : 0} captured exchanges`, 'success');
}

function downloadTrace() {
  const exchange = currentExchange();
  downloadJson(`simile-trace-${exchange.id}.json`, {
    exchange,
    journalEvents: relatedJournalEvents(exchange),
  });
  showToast(`Exported trace ${exchange.id}`, 'success');
}

function downloadFidelityReport() {
  downloadJson('simile-fidelity-report.json', {
    service: serviceData.service,
    generatedAt: new Date().toISOString(),
    metrics: serviceData.metrics,
    findings: serviceData.findings,
  });
  showToast('Exported the generated fidelity audit', 'success');
}

function exportRuntimeState() {
  if (!runtime.connected || !runtime.inspectionState) {
    showToast('Connect the local runtime before exporting state', 'warning');
    return;
  }
  downloadJson('simile-state.json', runtime.inspectionState);
  showToast('Exported live service state', 'success');
}

const icons = {
  logo: '<path d="M8.1 4.4a5.2 5.2 0 0 0 0 10.4h2.1v-2.7H8.1a2.5 2.5 0 1 1 0-5h5.8a2.5 2.5 0 1 1 0 5h-2.1v2.7h2.1a5.2 5.2 0 1 0 0-10.4H8.1Z"/><path d="M15.9 9.2h-2.1v2.7h2.1a2.5 2.5 0 1 1 0 5H10a2.5 2.5 0 1 1 0-5h2.1V9.2H10a5.2 5.2 0 1 0 0 10.4h5.8a5.2 5.2 0 1 0 0-10.4Z"/>',
  service: '<path d="M12 3 3.8 7.5 12 12l8.2-4.5L12 3Z"/><path d="m3.8 12 8.2 4.5 8.2-4.5M3.8 16.5 12 21l8.2-4.5"/>',
  operations: '<path d="M4 6h16M4 12h16M4 18h16"/><circle cx="7" cy="6" r="1"/><circle cx="17" cy="12" r="1"/><circle cx="10" cy="18" r="1"/>',
  exchanges: '<path d="M4 7h11M12 4l3 3-3 3M20 17H9M12 14l-3 3 3 3"/>',
  logs: '<path d="M5 6h14M5 12h14M5 18h14"/><circle cx="3" cy="6" r=".7"/><circle cx="3" cy="12" r=".7"/><circle cx="3" cy="18" r=".7"/>',
  state: '<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v7c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 12v7c0 1.7 3.6 3 8 3s8-1.3 8-3v-7"/>',
  deferred: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  scenarios: '<path d="M5 4h14v6H5zM5 14h14v6H5zM9 7h6M9 17h6"/>',
  faults: '<path d="m13 2-9 12h7l-1 8 9-12h-7l1-8Z"/>',
  fidelity: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="5"/><path d="m9.5 12 1.6 1.7 3.7-4"/>',
  sessions: '<path d="M4 5h16v14H4zM8 3v4M16 3v4M4 9h16"/><path d="m9 14 2 2 4-4"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.6v-.2h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"/>',
  docs: '<path d="M5 3h11a3 3 0 0 1 3 3v15H8a3 3 0 0 1-3-3V3Z"/><path d="M8 17h11M9 7h6M9 11h6"/>',
  chevron: '<path d="m9 18 6-6-6-6"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/>',
  filter: '<path d="M4 5h16l-6 7v5l-4 2v-7L4 5Z"/>',
  download: '<path d="M12 3v12M7 10l5 5 5-5M4 21h16"/>',
  trash: '<path d="M4 7h16M9 7V4h6v3M7 7l1 14h8l1-14M10 11v6M14 11v6"/>',
  copy: '<rect x="8" y="8" width="11" height="11" rx="1"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>',
  check: '<path d="m5 12 4 4L19 6"/>',
  warning: '<path d="M12 3 2.5 20h19L12 3Z"/><path d="M12 9v4M12 17h.01"/>',
  replay: '<path d="M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6"/>',
  terminal: '<path d="m4 6 5 5-5 5M11 18h9"/>',
  bell: '<path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4"/>',
  collapse: '<path d="m6 9 6 6 6-6"/>',
  maximize: '<path d="M8 3H3v5M16 3h5v5M8 21H3v-5M16 21h5v-5"/>',
  restore: '<path d="M8 3v5H3M16 3v5h5M8 21v-5H3M16 21v-5h5"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  moon: '<path d="M20.7 15.1A8.5 8.5 0 0 1 8.9 3.3 8.5 8.5 0 1 0 20.7 15.1Z"/>',
};

function icon(name, className = '') {
  return `<svg class="icon ${className}" viewBox="0 0 24 24" aria-hidden="true">${icons[name]}</svg>`;
}

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"]/g, (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[character]);
}

function syntaxJson(value) {
  return escapeHtml(value).replace(
    /(&quot;.*?&quot;)(?=\s*:)|(&quot;.*?&quot;)|\b(true|false|null)\b|\b(-?\d+(?:\.\d+)?)\b/g,
    (match, key, string, boolean, number) => {
      if (key) return `<span class="syntax-key">${key}</span>`;
      if (string) return `<span class="syntax-string">${string}</span>`;
      if (boolean) return `<span class="syntax-boolean">${boolean}</span>`;
      if (number) return `<span class="syntax-number">${number}</span>`;
      return match;
    },
  );
}

const hclBlockKeywords = new Set([
  'action', 'async', 'backend', 'behavior', 'collect', 'effect', 'fault', 'fault_profile',
  'issue', 'listen', 'openapi', 'outcome', 'principal', 'principal_setup', 'profile',
  'reporting', 'resolve', 'scenario', 'select', 'service', 'where',
]);

function syntaxHcl(value) {
  let output = '';
  let index = 0;
  const span = (className, token) => `<span class="${className}">${escapeHtml(token)}</span>`;

  while (index < value.length) {
    const remaining = value.slice(index);
    if (remaining[0] === '#' || remaining.startsWith('//')) {
      const end = value.indexOf('\n', index);
      const next = end === -1 ? value.length : end;
      output += span('syntax-comment', value.slice(index, next));
      index = next;
      continue;
    }
    if (remaining[0] === '"') {
      let end = index + 1;
      while (end < value.length) {
        if (value[end] === '"' && value[end - 1] !== '\\') {
          end += 1;
          break;
        }
        end += 1;
      }
      output += span('syntax-string', value.slice(index, end));
      index = end;
      continue;
    }
    const number = remaining.match(/^-?\d+(?:\.\d+)?/);
    if (number) {
      output += span('syntax-number', number[0]);
      index += number[0].length;
      continue;
    }
    const identifier = remaining.match(/^[A-Za-z_][A-Za-z0-9_-]*/);
    if (identifier) {
      const token = identifier[0];
      const trailing = remaining.slice(token.length);
      const className = ['true', 'false', 'null'].includes(token)
        ? 'syntax-boolean'
        : /^\s*=/.test(trailing)
          ? 'syntax-key'
          : hclBlockKeywords.has(token) || /^\s*(?:"[^"]*"\s*)*\{/.test(trailing)
            ? 'syntax-keyword'
            : '';
      output += className ? span(className, token) : escapeHtml(token);
      index += token.length;
      continue;
    }
    if ('{}[]=,.'.includes(remaining[0])) {
      output += span('syntax-punctuation', remaining[0]);
      index += 1;
      continue;
    }
    output += escapeHtml(remaining[0]);
    index += 1;
  }
  return output;
}

function route() {
  const raw = window.location.pathname.replace(/^\/+|\/+$/g, '') || 'service';
  const [section, detail, view] = raw.split('/').map((segment) => {
    try { return decodeURIComponent(segment); } catch { return segment; }
  });
  const known = navItems.some(([key]) => key === section) || ['logs', 'settings', 'docs'].includes(section);
  return { section: known ? section : 'service', detail, view };
}

function navigate(path) {
  const destination = path.startsWith('/') ? path : `/${path}`;
  if (window.location.pathname === destination && !window.location.search) return;
  window.history.pushState(null, '', destination);
  handleNavigation();
}

function handleNavigation() {
  state.serviceTab = 'overview';
  state.operationTab = 'contract';
  state.selectedOutcome = null;
  state.exchangeTab = 'timeline';
  renderShell();
  document.getElementById('workspace')?.focus({preventScroll:true});
  window.scrollTo(0, 0);
}

function badge(text, tone = 'neutral') {
  return `<span class="badge badge--${tone}">${text}</span>`;
}

function methodBadge(method) {
  return `<span class="method method--${method.toLowerCase()}">${method}</span>`;
}

function header(title, description, actions = '') {
  return `<header class="page-header"><div><h1>${title}</h1>${description ? `<p>${description}</p>` : ''}</div>${actions ? `<div class="page-actions">${actions}</div>` : ''}</header>`;
}

function toolbar({ search = 'Search', count = '', actions = '' } = {}) {
  return `<div class="toolbar"><label class="search-field">${icon('search')}<input type="search" placeholder="${search}" aria-label="${search}"></label><div class="toolbar__end">${count ? `<span class="result-count">${count}</span>` : ''}${actions}</div></div>`;
}

function tableHead(labels) {
  return `<thead><tr>${labels.map((label) => `<th>${label}</th>`).join('')}</tr></thead>`;
}

function localTabs(items, active, dataAttribute, ariaLabel, panelId) {
  return `<nav class="subtabs" role="tablist" aria-label="${ariaLabel}">${items.map(([key, label]) => `<button type="button" role="tab" id="${dataAttribute}-${key}-tab" class="${active === key ? 'is-active' : ''}" data-${dataAttribute}="${key}" aria-selected="${active === key}" aria-controls="${panelId}" tabindex="${active === key ? '0' : '-1'}">${label}</button>`).join('')}</nav>`;
}

function codePanel(title, code, options = {}) {
  return `<section class="code-panel ${options.className || ''}"><header><span>${title}</span><button class="icon-only" type="button" data-copy="${options.copy || title}" aria-label="Copy ${title}">${icon('copy')}</button></header><pre><code>${options.json === false ? code : syntaxJson(code)}</code></pre></section>`;
}

function renderShell() {
  let current = route();
  if (current.section === 'logs') {
    state.logDock = 'full';
    window.localStorage.setItem('simile-log-dock', state.logDock);
    window.history.replaceState(null, '', '/service');
    current = { section: 'service' };
  }
  const page = current;
  const app = document.getElementById('simile-web');
  app.className = `app app--logs-${state.logDock}`;
  app.innerHTML = `
    <header class="topbar">
      <div class="topbar__inner">
        <a class="product-brand" href="/service" aria-label="Simile home">${icon('logo', 'logo')}<strong>Simile</strong></a>
        <span class="topbar-divider" aria-hidden="true"></span>
        <button class="service-switcher" type="button" data-menu="service-switcher" aria-label="Switch local service" title="Switch local service"><span class="service-switcher__context">Service</span><strong>${serviceData.service.name}</strong><span class="service-revision">${serviceData.service.version}</span><span class="down-caret"></span></button>
        <div class="topbar__rail"><div class="local-context"><span class="${runtime.connected ? 'live-dot' : 'idle-dot'}"></span><strong>Local service</strong><span>${serviceData.service.listen}</span></div></div>
        <div class="topbar__actions">
          <button class="runtime-status" type="button" data-action="runtime"><span class="${runtime.connected ? 'live-dot' : 'idle-dot'}"></span>${runtime.connected ? 'Connected' : 'Disconnected'}</button>
          <button class="environment" type="button" data-menu="environment"><span class="clock-face"></span>${state.environment}<span class="down-caret"></span></button>
          <button class="top-icon sound-toggle ${state.soundEnabled ? 'is-active' : ''}" type="button" data-action="toggle-sound" aria-label="${state.soundEnabled ? 'Mute' : 'Enable'} issue sounds" title="${state.soundEnabled ? 'Issue sounds enabled' : 'Issue sounds muted'}">${icon('bell')}</button>
          <button class="top-icon theme-toggle" type="button" data-action="toggle-theme" aria-label="Switch to ${state.theme === 'dark' ? 'light' : 'dark'} mode" title="Switch to ${state.theme === 'dark' ? 'light' : 'dark'} mode">${icon(state.theme === 'dark' ? 'sun' : 'moon')}</button>
          <button class="top-icon" type="button" data-action="terminal" aria-label="Open terminal">${icon('terminal')}</button>
          <a class="top-icon" href="/docs" aria-label="Documentation">?</a>
          <button class="avatar" type="button" data-menu="profile">YA</button>
        </div>
      </div>
    </header>
    <div class="product-nav">
      <div class="product-nav__inner">
        <nav class="primary-nav" aria-label="Primary navigation">
          ${navItems.map(([key, label]) => `<a href="/${key}" class="nav-link ${page.section === key ? 'is-active' : ''}" ${page.section === key ? 'aria-current="page"' : ''}>${icon(key)}<span>${label}</span>${key === 'deferred' ? '<span class="nav-count">0</span>' : ''}</a>`).join('')}
        </nav>
        <nav class="secondary-nav" aria-label="Support navigation">
          <a href="/docs" class="nav-link ${page.section === 'docs' ? 'is-active' : ''}">${icon('docs')}<span>Docs</span></a>
        </nav>
      </div>
    </div>
    <main id="workspace" class="workspace" tabindex="-1">${renderPage(page)}</main>
    ${logDock()}
    <div class="popover" data-popover hidden></div>
  `;
  bindInteractions();
}

function renderPage(current) {
  if (current.section === 'exchanges' && current.detail && current.view === 'report') return exchangeReportPage();
  if (current.section === 'exchanges' && current.detail) return exchangeDetail();
  if (current.section === 'operations' && current.detail) return operationDetail(current.detail);
  const pages = {
    service: servicePage,
    operations: operationsPage,
    exchanges: exchangesPage,
    state: statePage,
    deferred: deferredPage,
    scenarios: scenariosPage,
    faults: faultsPage,
    fidelity: fidelityPage,
    sessions: sessionsPage,
    settings: settingsPage,
    docs: docsPage,
  };
  return pages[current.section]();
}

function runtimeAnalyticsData() {
  const bucketCount = 16;
  const exchanges = runtime.connected ? [...runtime.exchanges].sort((left, right) => left.startedAtMs - right.startedAtMs) : [];
  const events = runtime.connected ? [...(runtime.inspectionState?.events ?? [])].sort((left, right) => left.at - right.at) : [];
  const timestamps = [...exchanges.map((exchange) => exchange.startedAtMs), ...events.map((event) => event.at)].filter(Number.isFinite);
  const now = Date.now();
  const start = timestamps.length ? Math.min(...timestamps) : now - 15 * 60_000;
  const observedEnd = timestamps.length ? Math.max(...timestamps) : now;
  const end = Math.max(observedEnd, start + 60_000);
  const duration = end - start;
  const requestCounts = Array(bucketCount).fill(0);
  const latencyTotals = Array(bucketCount).fill(0);
  const latencyCounts = Array(bucketCount).fill(0);
  const journalCounts = Array(bucketCount).fill(0);
  const statuses = Array.from({ length: bucketCount }, () => [0, 0, 0, 0]);
  const operations = new Map();
  const bucket = (timestamp) => Math.min(bucketCount - 1, Math.max(0, Math.floor(((timestamp - start) / duration) * bucketCount)));

  exchanges.forEach((exchange) => {
    const index = bucket(exchange.startedAtMs);
    const latency = exchange.durationMicros / 1000;
    requestCounts[index] += 1;
    latencyTotals[index] += latency;
    latencyCounts[index] += 1;
    const statusGroup = Math.min(3, Math.max(0, Math.floor(Number(exchange.status) / 100) - 2));
    statuses[index][statusGroup] += 1;
    const operation = operationIdByKey.get(exchange.operation) || exchange.operation;
    operations.set(operation, (operations.get(operation) ?? 0) + 1);
  });
  events.forEach((event) => { journalCounts[bucket(event.at)] += 1; });

  const latencies = exchanges.map((exchange) => exchange.durationMicros / 1000).sort((left, right) => left - right);
  const p95Index = Math.max(0, Math.ceil(latencies.length * .95) - 1);
  const formatTime = (timestamp) => new Intl.DateTimeFormat([], { hour: '2-digit', minute: '2-digit' }).format(new Date(timestamp));
  const xLabels = [0, .25, .5, .75, 1].map((ratio) => formatTime(start + duration * ratio));
  return {
    connected: runtime.connected,
    exchanges,
    requestCounts,
    averageLatency: latencyTotals.map((total, index) => latencyCounts[index] ? total / latencyCounts[index] : 0),
    journalCounts,
    statuses,
    operations: [...operations.entries()].sort((left, right) => right[1] - left[1]),
    totalRequests: exchanges.length,
    totalEvents: events.length,
    issueCount: exchanges.filter((exchange) => Number(exchange.status) >= 400).length,
    p95: latencies.length ? latencies[p95Index] : 0,
    xLabels,
    windowLabel: timestamps.length ? `${xLabels[0]}–${xLabels.at(-1)}` : 'No captured activity',
  };
}

function compactNumber(value) {
  return new Intl.NumberFormat([], { notation: value >= 1000 ? 'compact' : 'standard', maximumFractionDigits: 1 }).format(value);
}

function formatMilliseconds(value) {
  return value < 10 ? `${value.toFixed(1)}ms` : `${Math.round(value)}ms`;
}

function analyticsLineChart(values, { label, max, yLabels, area = true, xLabels, empty = false } = {}) {
  const width = 720;
  const height = 188;
  const padding = 4;
  const ceiling = max || Math.max(...values, 1);
  const floor = Math.min(0, ...values);
  const range = Math.max(1, ceiling - floor);
  const points = values.map((value, index) => {
    const x = padding + (index / (values.length - 1)) * (width - padding * 2);
    const y = padding + ((ceiling - value) / range) * (height - padding * 2);
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(' ');
  const areaPoints = `${padding},${height - padding} ${points} ${width - padding},${height - padding}`;
  const labels = yLabels || [String(ceiling), String(Math.round(ceiling / 2)), '0'];
  const axisLabels = xLabels ?? ['', '', '', '', ''];
  return `<div class="analytics-chart ${empty ? 'is-empty' : ''}" role="img" aria-label="${label}">
    <div class="analytics-gridlines" aria-hidden="true"></div>
    <div class="analytics-y-axis" aria-hidden="true"><span>${labels[0]}</span><span>${labels[1]}</span><span>${labels[2]}</span></div>
    <svg viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" aria-hidden="true">${area ? `<polygon class="analytics-area" points="${areaPoints}"/>` : ''}<polyline class="analytics-line" points="${points}"/></svg>
    ${empty ? '<span class="analytics-empty">No captured activity</span>' : ''}
    <div class="analytics-x-axis" aria-hidden="true">${axisLabels.map((axisLabel) => `<span>${axisLabel}</span>`).join('')}</div>
  </div>`;
}

function analyticsStatusChart(groups, { xLabels, empty = false } = {}) {
  const width = 720;
  const height = 188;
  const ceiling = Math.max(...groups.map((segments) => segments.reduce((total, value) => total + value, 0)), 1);
  const gap = 13;
  const barWidth = (width - gap * (groups.length - 1)) / groups.length;
  const bars = groups.map((segments, index) => {
    let bottom = height;
    return segments.map((value, segmentIndex) => {
      const segmentHeight = (value / ceiling) * height;
      bottom -= segmentHeight;
      return `<rect class="analytics-status analytics-status--${segmentIndex + 1}" x="${(index * (barWidth + gap)).toFixed(1)}" y="${bottom.toFixed(1)}" width="${barWidth.toFixed(1)}" height="${segmentHeight.toFixed(1)}"/>`;
    }).join('');
  }).join('');
  return `<div class="analytics-chart ${empty ? 'is-empty' : ''}" role="img" aria-label="HTTP response status codes in retained exchange history">
    <div class="analytics-gridlines" aria-hidden="true"></div>
    <div class="analytics-y-axis" aria-hidden="true"><span>${ceiling}</span><span>${Math.round(ceiling / 2)}</span><span>0</span></div>
    <svg viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" aria-hidden="true">${bars}</svg>
    ${empty ? '<span class="analytics-empty">No captured responses</span>' : ''}
    <div class="analytics-x-axis" aria-hidden="true">${(xLabels ?? ['', '', '', '', '']).map((axisLabel) => `<span>${axisLabel}</span>`).join('')}</div>
  </div>`;
}

function analyticsLegend(items) {
  return `<div class="analytics-legend">${items.map(([label, value, tone = 'blue']) => `<span><i class="legend-dot legend-dot--${tone}"></i>${label}<strong>${value}</strong></span>`).join('')}</div>`;
}

function contractMetricBars(entries, { limit = 8 } = {}) {
  const ranked = [...entries].sort((left, right) => right[1] - left[1]).slice(0, limit);
  const maximum = Math.max(...ranked.map(([, value]) => value), 1);
  return `<div class="contract-bars">${ranked.map(([label, value]) => `<div><code>${label}</code><span><i style="--rank:${(value / maximum) * 100}%"></i></span><strong>${value}</strong></div>`).join('')}</div>`;
}

function servicePage() {
  return `<div class="page">${header(serviceData.service.name, serviceData.service.description, `<button class="button button--primary" type="button" data-action="open-console">Open console</button><button class="button" type="button" disabled title="Restart the service from its owning terminal">Restart</button>`)}
    <div class="status-strip"><div>${badge(runtime.connected ? 'RUNNING' : 'OFFLINE', runtime.connected ? 'success' : 'neutral')}<strong>${runtime.connected ? `Listening on ${serviceData.service.listen}` : `Waiting for ${serviceData.service.listen}`}</strong></div><dl><div><dt>Contract</dt><dd>${serviceData.service.version}</dd></div><div><dt>OpenAPI</dt><dd>${serviceData.service.openapiVersion}</dd></div><div><dt>Profiles</dt><dd>${serviceData.metrics.profiles}</dd></div><div><dt>Artifact</dt><dd><code>sha256:${serviceData.artifact.sha256.slice(0, 8)}…${serviceData.artifact.sha256.slice(-4)}</code></dd></div></dl></div>
    ${localTabs([['overview', 'Overview'], ['stack', 'Stack'], ['operations', 'Operations'], ['state', 'State'], ['fidelity', 'Fidelity']], state.serviceTab, 'service-tab', 'Service views', 'service-tab-panel')}
    <div id="service-tab-panel" class="local-tab-panel" role="tabpanel" aria-labelledby="service-tab-${state.serviceTab}-tab">${serviceTabPanel()}</div>
  </div>`;
}

function serviceTabPanel() {
  const panels = {
    overview: serviceOverviewPanel,
    stack: serviceStackPanel,
    operations: serviceOperationsPanel,
    state: serviceStatePanel,
    fidelity: serviceFidelityPanel,
  };
  return panels[state.serviceTab]();
}

function serviceStackPanel() {
  const entries = Object.entries(stackComponents);
  const selected = stackComponents[state.selectedStackComponent];
  return `<section class="stack-view">
    <dl class="stack-overview" aria-label="Stack summary">
      <div><dt>Artifact protocol</dt><dd><code>v${serviceData.artifact.formatVersion}</code></dd></div>
      <div><dt>Portable backends</dt><dd>${entries.length}</dd></div>
      <div><dt>Async operations</dt><dd>${serviceData.asyncOperations.length}</dd></div>
      <div><dt>Consumer profiles</dt><dd>${serviceData.profiles.length}</dd></div>
      <div><dt>Export</dt><dd>${serviceData.artifact.publicExportSafe ? 'Public-safe' : 'Restricted'}</dd></div>
    </dl>
    <div class="stack-workbench">
      <section class="stack-list" aria-label="Stack components">
        <header><div><h2>Backends</h2><p>Portable capabilities declared by this service.</p></div><span>${entries.length}</span></header>
        <div class="stack-list__rows">${entries.map(([key, component]) => `<button type="button" class="stack-row ${state.selectedStackComponent === key ? 'is-active' : ''}" data-stack-component="${key}" aria-pressed="${state.selectedStackComponent === key}"><span class="stack-row__icon">${icon(component.icon)}</span><span><strong>${component.label}</strong><code>${component.process}</code></span><span><small>${component.activity}</small>${badge(component.status, 'neutral')}</span>${icon('chevron')}</button>`).join('')}</div>
      </section>
      <section class="stack-detail" aria-live="polite">
        <header class="stack-detail__header"><div class="stack-detail__identity"><span class="stack-row__icon">${icon(selected.icon)}</span><div><span>${selected.process}</span><h2>${selected.label}</h2><p>${selected.description}</p></div></div>${badge(selected.status)}</header>
        <dl class="stack-facts">
          <div><dt>Reference</dt><dd><code>${selected.label}</code></dd></div>
          <div><dt>Execution</dt><dd>In-process capability</dd></div>
          <div><dt>Persistence</dt><dd>Checkpointed with service state</dd></div>
          <div><dt>Async consumers</dt><dd>${selected.workers.length}</dd></div>
          <div><dt>Artifact format</dt><dd><code>v${serviceData.artifact.formatVersion}</code></dd></div>
          <div><dt>Compiler</dt><dd><code>${serviceData.artifact.compilerVersion}</code></dd></div>
        </dl>
        <section class="stack-resource-panel"><header><div><h3>Attached asynchronous work</h3><p>Workers that depend on this backend.</p></div></header>${selected.workers.length ? `<table class="data-table">${tableHead(['Operation', 'Description', 'Backends'])}<tbody>${selected.workers.map((worker) => `<tr><td><code>${worker.name}</code></td><td>${worker.description}</td><td>${worker.backends.map((backend) => `<code>${backend}</code>`).join(' ')}</td></tr>`).join('')}</tbody></table>` : `<div class="stack-empty">No asynchronous worker depends on this backend. It stores public resource snapshots used by stateful operations.</div>`}</section>
        <footer class="stack-protocol"><span>${icon('check')}Portable contract</span><span>${icon('check')}Serializable and restorable</span><span>${icon('check')}Declared in the Simfile</span></footer>
      </section>
    </div>
  </section>`;
}

function serviceOverviewPanel() {
  const analytics = runtimeAnalyticsData();
  const requestCeiling = Math.max(...analytics.requestCounts, 1);
  const latencyCeiling = Math.max(...analytics.averageLatency, 1);
  const journalCeiling = Math.max(...analytics.journalCounts, 1);
  const twoHundreds = analytics.exchanges.filter((exchange) => Number(exchange.status) >= 200 && Number(exchange.status) < 300).length;
  const threeHundreds = analytics.exchanges.filter((exchange) => Number(exchange.status) >= 300 && Number(exchange.status) < 400).length;
  const fourHundreds = analytics.exchanges.filter((exchange) => Number(exchange.status) >= 400 && Number(exchange.status) < 500).length;
  const fiveHundreds = analytics.exchanges.filter((exchange) => Number(exchange.status) >= 500).length;
  const empty = analytics.totalRequests === 0;
  return `
    <div class="runtime-analytics-heading"><div><span class="eyebrow">Runtime telemetry</span><h2>Captured service activity</h2><p>${analytics.connected ? analytics.windowLabel : `Waiting for ${localServiceUrl}`}</p></div><a class="text-link" href="/exchanges">Inspect exchanges</a></div>
    <section class="analytics-grid" aria-label="Live service metrics">
      <article class="analytics-panel">
        <header class="analytics-panel__header"><div><span class="analytics-title">Total requests</span><div class="analytics-kpi"><strong>${analytics.totalRequests}</strong></div></div><span class="analytics-context">retained locally</span></header>
        ${analyticsLegend([['Requests', analytics.totalRequests]])}
        ${analyticsLineChart(analytics.requestCounts, { label: `${analytics.totalRequests} captured requests over time`, max: requestCeiling, yLabels: [String(requestCeiling), String(Math.round(requestCeiling / 2)), '0'], xLabels: analytics.xLabels, empty })}
      </article>
      <article class="analytics-panel">
        <header class="analytics-panel__header"><div><span class="analytics-title">Request latency</span><div class="analytics-kpi"><strong>${formatMilliseconds(analytics.p95)}</strong></div></div><span class="analytics-context">p95</span></header>
        ${analyticsLegend([['Average latency', analytics.p95 ? formatMilliseconds(analytics.p95) : '0ms']])}
        ${analyticsLineChart(analytics.averageLatency, { label: `Request latency with p95 ${formatMilliseconds(analytics.p95)}`, max: latencyCeiling, yLabels: [formatMilliseconds(latencyCeiling), formatMilliseconds(latencyCeiling / 2), '0ms'], xLabels: analytics.xLabels, area: false, empty })}
      </article>
      <article class="analytics-panel">
        <header class="analytics-panel__header"><div><span class="analytics-title">Journal activity</span><div class="analytics-kpi"><strong>${compactNumber(analytics.totalEvents)}</strong></div></div><span class="analytics-context">causal events</span></header>
        ${analyticsLegend([['Journal events', compactNumber(analytics.totalEvents), 'teal']])}
        ${analyticsLineChart(analytics.journalCounts, { label: `${analytics.totalEvents} journal events over time`, max: journalCeiling, yLabels: [compactNumber(journalCeiling), compactNumber(Math.round(journalCeiling / 2)), '0'], xLabels: analytics.xLabels, area: true, empty: analytics.totalEvents === 0 })}
      </article>
      <article class="analytics-panel">
        <header class="analytics-panel__header"><div><span class="analytics-title">Status codes</span><div class="analytics-kpi"><strong>${analytics.totalRequests}</strong></div></div><span class="analytics-context">${analytics.issueCount} issues</span></header>
        ${analyticsLegend([['2xx', twoHundreds], ['3xx', threeHundreds, 'amber'], ['4xx', fourHundreds, 'pink'], ['5xx', fiveHundreds, 'teal']])}
        ${analyticsStatusChart(analytics.statuses, { xLabels: analytics.xLabels, empty })}
      </article>
    </section>
    <section class="analytics-panel analytics-panel--ranking live-operation-ranking"><header class="analytics-panel__header"><div><span class="analytics-title">Operations</span><p>Requests grouped by operation across retained history.</p></div><span class="analytics-context">${analytics.operations.length} observed</span></header>${analytics.operations.length ? contractMetricBars(analytics.operations, { limit: 10 }) : '<div class="analytics-ranking-empty">Operations appear here after the service receives a request.</div>'}</section>
  `;
}

function serviceOperationsPanel() {
  const rows = operationRows.slice(0, 12);
  return `<section class="table-panel"><div class="panel-heading panel-heading--table"><div><h2>Operations</h2><p>Callable operations compiled for this service.</p></div><a class="text-link" href="/operations">Open operations workspace</a></div><table class="data-table operations-table">${tableHead(['Method', 'Path', 'Operation ID', 'Success', 'Outcomes', 'Behavior'])}<tbody>${rows.map(([method, path, id, success, outcomes, behavior]) => `<tr class="clickable-row" data-operation="${id}" tabindex="0"><td>${methodBadge(method)}</td><td><code>${path}</code></td><td><code>${id}</code></td><td><span class="status-code status-code--good">${success}</span></td><td>${outcomes}</td><td>${behavior}</td></tr>`).join('')}</tbody></table><footer class="table-footer"><span>Showing ${rows.length} of ${serviceData.metrics.operations} compiled operations</span><a class="text-link" href="/operations">View all operations</a></footer></section>`;
}

function serviceStatePanel() {
  if (runtime.connected && runtime.status) {
    return `<section class="runtime-boundary-panel runtime-boundary-panel--connected">
      <div class="runtime-boundary-panel__icon">${icon('state')}</div>
      <div><span class="eyebrow">Live runtime inspection</span><h2>${runtime.status.resources} resources and ${runtime.status.events} journal events</h2><p>The running service owns the checkpoint and exposes this read-only view directly from its in-memory Stack.</p></div>
      <a class="button" href="/state">Inspect live state</a>
    </section>`;
  }
  return `<section class="runtime-boundary-panel">
    <div class="runtime-boundary-panel__icon">${icon('state')}</div>
    <div><span class="eyebrow">Runtime unavailable</span><h2>Start ${serviceName} to inspect state</h2><p>The interface will connect to the read-only loopback inspector at <code>${localServiceUrl}/.simile/inspection</code>.</p></div>
    <a class="button" href="/state">View disconnected state UI</a>
  </section>`;
}

function serviceFidelityPanel() {
  const metrics = serviceData.metrics;
  return `<div class="service-fidelity-panel"><div class="quality-grid"><div><strong>${metrics.operations} / ${metrics.operations}</strong><span>Operations executable</span></div><div><strong>${metrics.providerAuthoredOutcomes}</strong><span>Provider-authored outcomes</span></div><div><strong>${metrics.schemaSynthesizedOutcomes}</strong><span>Schema-synthesized outcomes</span></div><div><strong>${metrics.errors}</strong><span>Audit errors</span></div></div><section class="table-panel"><div class="panel-heading panel-heading--table"><div><h2>Remaining gaps</h2><p>Current findings reported by the compiled artifact audit.</p></div><a class="text-link" href="/fidelity">Open fidelity workspace</a></div><table class="data-table">${tableHead(['Severity', 'Code', 'Subject', 'Finding'])}<tbody>${fidelityFindingRows(8)}</tbody></table></section></div>`;
}

function fidelityFindingRows(limit = serviceData.findings.length) {
  return serviceData.findings.slice(0, limit).map((finding) => {
    const operationId = operationIdByKey.get(finding.subject);
    const subject = operationId ? `<a class="operation-link" href="/operations/${operationId}"><code>${finding.subject}</code></a>` : `<code>${finding.subject}</code>`;
    return `<tr><td>${badge(finding.severity.toUpperCase(), finding.severity === 'error' ? 'danger' : 'warning')}</td><td><code>${finding.code}</code></td><td>${subject}</td><td>${finding.message}</td></tr>`;
  }).join('');
}

function sessionsPage() {
  const writable = Boolean(runtime.control?.writable);
  const sessions = runtime.sessions ?? [];
  const events = [...(runtime.controlEvents ?? [])].reverse();
  return `<div class="page sessions-page">
    ${header('Test sessions', 'Name a known service state, restore it later, and keep every control action visible.', `<button class="button button--primary" type="button" data-action="create-session" ${writable ? '' : 'disabled'}>${icon('check')}Save checkpoint</button>`)}
    <section class="session-command-bar">
      <label><span>Checkpoint name</span><input data-session-name placeholder="checkout-ready" autocomplete="off" spellcheck="false"></label>
      <div><label><span>Advance manual clock</span><input data-clock-advance value="1000" inputmode="numeric"><small>milliseconds</small></label><button class="button" type="button" data-action="advance-clock" ${writable ? '' : 'disabled'}>Advance</button></div>
      <span class="protocol-status">${badge(runtime.control ? `PROTOCOL v${runtime.control.version}` : 'OFFLINE', runtime.control ? 'success' : 'warning')} ${writable ? 'Mutations are authorized for this console.' : 'Attach through simile console to enable controls.'}</span>
    </section>
    <div class="session-layout">
      <section class="table-panel session-checkpoints"><div class="panel-heading panel-heading--table"><div><h2>Checkpoints</h2><p>Each checkpoint contains the complete Stack, clock, replay identities, and causal journal.</p></div><span class="panel-stat">${sessions.length}</span></div>${sessions.length ? `<table class="data-table">${tableHead(['Name','Captured','Logical time','Exchanges','Actions'])}<tbody>${sessions.map((session) => `<tr><td><code>${escapeHtml(session.name)}</code></td><td>${new Date(session.createdAtMs).toLocaleString()}</td><td><code>${new Date(session.logicalTimeMilliseconds).toISOString()}</code></td><td>${session.exchangeCount}</td><td><span class="session-actions"><button class="button button--compact" data-restore-session="${escapeHtml(session.name)}">Restore</button><button class="button button--compact button--quiet-danger" data-delete-session="${escapeHtml(session.name)}">Delete</button></span></td></tr>`).join('')}</tbody></table>` : `<section class="empty-state">${icon('state')}<h2>No checkpoints yet</h2><p>Save the current state before a test, then restore it for the next run.</p></section>`}</section>
      <section class="control-events"><header><div><h2>Control history</h2><p>Workbench, replay, state, and clock actions from this runtime.</p></div>${badge(`${events.length} EVENTS`)}</header>${events.length ? `<ol>${events.map((event) => `<li><span class="event-kind">${escapeHtml(event.kind)}</span><div><strong>${escapeHtml(event.summary)}</strong><small>${new Date(event.atMs).toLocaleTimeString()}</small></div><code>#${event.sequence}</code></li>`).join('')}</ol>` : `<div class="workbench-empty">${icon('exchanges')}<strong>No control actions yet</strong><span>Actions appear here in sequence.</span></div>`}</section>
    </div>
  </div>`;
}

async function createSession() {
  const name = document.querySelector('[data-session-name]')?.value.trim();
  try {
    const session = await inspectorJson('/.simile/console/v1/sessions', {
      method: 'POST',
      body: JSON.stringify(name ? { name } : {}),
    });
    await refreshRuntime(true);
    showToast(`Saved checkpoint ${session.name}`, 'success');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

async function restoreSession(name) {
  if (!window.confirm(`Restore ${name}? The current service state will be replaced.`)) return;
  try {
    await inspectorJson(`/.simile/console/v1/sessions/${encodeURIComponent(name)}/restore`, {
      method: 'POST',
      body: JSON.stringify({ confirm: true }),
    });
    await refreshRuntime(true);
    showToast(`Restored checkpoint ${name}`, 'success');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

async function deleteSession(name) {
  if (!window.confirm(`Delete checkpoint ${name}?`)) return;
  try {
    await inspectorJson(`/.simile/console/v1/sessions/${encodeURIComponent(name)}`, { method: 'DELETE' });
    await refreshRuntime(true);
    showToast(`Deleted checkpoint ${name}`, 'success');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

async function advanceSessionClock() {
  const milliseconds = Number(document.querySelector('[data-clock-advance]')?.value);
  if (!Number.isSafeInteger(milliseconds) || milliseconds <= 0) {
    showToast('Enter a positive number of milliseconds', 'warning');
    return;
  }
  try {
    const result = await inspectorJson('/.simile/console/v1/clock/advance', {
      method: 'POST',
      body: JSON.stringify({ milliseconds }),
    });
    await refreshRuntime(true);
    showToast(`Clock advanced to ${new Date(result.now).toISOString()}`, 'success');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

function operationsPage() {
  return `<div class="page">${header('Operations', `${serviceData.metrics.operations} callable operations compiled from the active contract`, `<button class="button" data-action="download-contract">${icon('download')}Export contract</button>`)}${toolbar({ search: 'Search paths, operation IDs, or domains', count: `${serviceData.metrics.operations} operations`, actions: `<button class="button button--compact" data-action="filter-operations">${icon('filter')}Filter</button>` })}<section class="table-panel"><table class="data-table operations-table">${tableHead(['Method', 'Path', 'Operation ID', 'Success', 'Outcomes', 'Behavior'])}<tbody>${operationRows.map(([method,path,id,success,outcomes,behavior]) => `<tr class="clickable-row" data-operation="${id}" tabindex="0"><td>${methodBadge(method)}</td><td><code>${path}</code></td><td><code>${id}</code></td><td><span class="status-code status-code--good">${success}</span></td><td>${outcomes}</td><td>${behavior}</td></tr>`).join('')}</tbody></table><footer class="table-footer"><span data-list-result-count>Showing all ${serviceData.metrics.operations} compiled operations</span></footer></section></div>`;
}

function preparedOperationRequest(operationId) {
  const row = operationRows.find(([, , id]) => id === operationId);
  const detail = operationCatalog[operationId];
  if (!row || !detail) return null;
  const [method, path] = row;
  const example = detail.request ? JSON.parse(detail.request) : {};
  const pathKeys = [...path.matchAll(/\{([^}]+)\}/g)].map((match) => match[1]);
  const fallbackValues = { order_id: 'or_9kjy94hjk' };
  const pathParameters = Object.fromEntries(pathKeys.map((key) => [key, example[key] ?? fallbackValues[key] ?? `example_${key}`]));
  const resolvedPath = path.replace(/\{([^}]+)\}/g, (_, key) => encodeURIComponent(pathParameters[key]));
  const requestFields = Object.fromEntries(Object.entries(example).filter(([key]) => !pathKeys.includes(key)));
  const query = ['GET', 'HEAD'].includes(method) ? requestFields : {};
  const hasRequestBody = detail.requestSchema && detail.requestSchema !== 'No request body';
  const body = ['GET', 'HEAD'].includes(method) || !hasRequestBody ? null : requestFields;
  const search = new URLSearchParams(Object.entries(query).map(([key, value]) => [key, typeof value === 'string' ? value : JSON.stringify(value)])).toString();
  const url = `${localServiceUrl}${resolvedPath}${search ? `?${search}` : ''}`;
  return { operationId, method, resolvedPath, url, query, body };
}

function requestForFormat(operationId, format) {
  const request = preparedOperationRequest(operationId);
  if (!request) return null;
  const bodyJson = request.body ? JSON.stringify(request.body, null, 2) : null;
  if (format === 'curl') {
    const body = bodyJson ? ` \\\n+  --header 'Content-Type: application/json' \\\n+  --data '${bodyJson.replaceAll("'", "'\\''")}'` : '';
    return `curl --request ${request.method} '${request.url}'${body}`;
  }
  if (format === 'sdk') {
    const sdkRequest = {
      operationId: request.operationId,
      method: request.method,
      path: request.resolvedPath,
      ...(Object.keys(request.query).length ? { query: request.query } : {}),
      ...(request.body ? { body: request.body } : {}),
    };
    return `// Use with your generated OpenAPI SDK client.\nconst response = await client.request(${JSON.stringify(sdkRequest, null, 2)});`;
  }
  if (format === 'insomnia') {
    const workspaceId = 'wrk_simile_local';
    return JSON.stringify({
      _type: 'export',
      __export_format: 4,
      __export_date: '2026-11-23T10:11:33.000Z',
      __export_source: 'simile.web:v1',
      resources: [
        { _id: workspaceId, _type: 'workspace', name: 'Simile local service', description: 'Requests prepared from the active Simile contract', scope: 'collection' },
        { _id: `env_${request.operationId}`, _type: 'environment', parentId: workspaceId, name: 'Local', data: { base_url: localServiceUrl } },
        {
          _id: `req_${request.operationId}`,
          _type: 'request',
          parentId: workspaceId,
          name: request.operationId,
          method: request.method,
          url: request.url.replace(localServiceUrl, '{{ _.base_url }}'),
          headers: bodyJson ? [{ name: 'Content-Type', value: 'application/json' }] : [],
          body: bodyJson ? { mimeType: 'application/json', text: bodyJson } : {},
          parameters: [],
        },
      ],
    }, null, 2);
  }
  if (format === 'postman') {
    const parsedUrl = new URL(request.url);
    return JSON.stringify({
      info: {
        _postman_id: `simile-${request.operationId}`,
        name: `Simile · ${request.operationId}`,
        schema: 'https://schema.getpostman.com/json/collection/v2.1.0/collection.json',
      },
      variable: [{ key: 'base_url', value: localServiceUrl }],
      item: [{
        name: request.operationId,
        request: {
          method: request.method,
          header: bodyJson ? [{ key: 'Content-Type', value: 'application/json', type: 'text' }] : [],
          ...(bodyJson ? { body: { mode: 'raw', raw: bodyJson, options: { raw: { language: 'json' } } } } : {}),
          url: {
            raw: request.url.replace(localServiceUrl, '{{base_url}}'),
            host: ['{{base_url}}'],
            path: parsedUrl.pathname.split('/').filter(Boolean),
            query: [...parsedUrl.searchParams].map(([key, value]) => ({ key, value })),
          },
        },
      }],
    }, null, 2);
  }
  return null;
}

async function copyPreparedRequest(format) {
  const content = requestForFormat(route().detail, format);
  if (!content) return;
  try {
    await navigator.clipboard.writeText(content);
    showToast(`Copied ${requestFormats[format].toast}`, 'success');
  } catch {
    showToast('Clipboard access is unavailable', 'warning');
  }
}

function operationDetail(operationId) {
  const row = operationRows.find(([, , id]) => id === operationId);
  const detail = operationCatalog[operationId];
  if (!row || !detail) return `<div class="page">${header('Operation not found', 'The requested operation is not present in this compiled contract.')}<a class="button" href="/operations">Back to operations</a></div>`;
  const [method, path, , success, outcomes, behavior] = row;
  const tabs = [['contract', 'Contract'], ['try', 'Try it'], ['exchanges', 'Recent exchanges'], ['scenarios', 'Scenarios']];
  return `<div class="page operation-page"><a class="back-link" href="/operations">‹ Back to Operations</a><div class="operation-heading"><div><div class="title-line">${methodBadge(method)}<h1>${operationId}</h1></div><p>${detail.summary}</p></div><div class="request-copy-control"><button class="button button--primary request-copy-action" type="button" data-action="copy-request">${icon('copy')}<span>${requestFormats[state.requestCopyFormat].label}</span></button><button class="button button--primary request-copy-menu-button" type="button" data-menu="copy-request" aria-label="Choose request copy format" aria-haspopup="menu" aria-expanded="false"><span class="down-caret" aria-hidden="true"></span></button></div></div><dl class="operation-summary"><div><dt>Path</dt><dd><code>${path}</code></dd></div><div><dt>Success</dt><dd><span class="status-code status-code--good">${success}</span></dd></div><div><dt>Behavior</dt><dd>${behavior}</dd></div><div><dt>Outcomes</dt><dd>${outcomes}</dd></div><div><dt>Request schema</dt><dd>${detail.requestSchema}</dd></div><div><dt>Response schema</dt><dd>${detail.responseSchema}</dd></div></dl>${localTabs(tabs, state.operationTab, 'operation-tab', 'Operation views', 'operation-tab-panel')}<div id="operation-tab-panel" class="local-tab-panel" role="tabpanel" aria-labelledby="operation-tab-${state.operationTab}-tab">${operationTabPanel(operationId, detail)}</div></div>`;
}

function operationTabPanel(operationId, detail) {
  if (state.operationTab === 'try') return operationWorkbenchPanel(operationId);
  if (state.operationTab === 'exchanges') return operationExchangesPanel(operationId);
  if (state.operationTab === 'scenarios') return operationScenariosPanel(operationId);
  return `<section class="operation-grid">${codePanel('Representative request', detail.request, {copy:`${operationId} request`,className:'code-panel--large'})}${codePanel('Representative response', detail.response, {copy:`${operationId} response`,className:'code-panel--large'})}</section>${operationOutcomesPanel(operationId, detail.outcomes)}`;
}

function workbenchDraft(operationId) {
  if (state.workbenchDrafts[operationId]) {
    const draft = state.workbenchDrafts[operationId];
    draft.query = normalizeWorkbenchPairs(draft.query);
    draft.headers = normalizeWorkbenchPairs(draft.headers);
    return draft;
  }
  const prepared = preparedOperationRequest(operationId);
  const draft = {
    path: prepared?.resolvedPath ?? '/',
    query: normalizeWorkbenchPairs(prepared?.query),
    headers: [],
    body: prepared?.body == null ? '' : JSON.stringify(prepared.body, null, 2),
    scenario: '',
    faultProfile: '',
    profile: activeProfiles[0]?.name ?? '',
    seed: '42',
  };
  state.workbenchDrafts[operationId] = draft;
  return draft;
}

function normalizeWorkbenchPairs(value) {
  if (Array.isArray(value)) return value.map((pair) => ({ key: String(pair.key ?? ''), value: String(pair.value ?? '') }));
  if (typeof value === 'string') {
    try { return normalizeWorkbenchPairs(JSON.parse(value || '{}')); } catch { return []; }
  }
  if (!value || typeof value !== 'object') return [];
  return Object.entries(value).flatMap(([key, item]) => (Array.isArray(item) ? item : [item]).map((entry) => ({ key, value: String(entry) })));
}

function workbenchPairEditor(label, kind, pairs) {
  const rows = pairs.length ? pairs : [{ key: '', value: '' }];
  return `<fieldset class="key-value-editor">
    <legend>${label}</legend>
    <div class="key-value-list">
      ${rows.map((pair, index) => `<div class="key-value-row" data-pair-row="${kind}">
        <input data-pair-key aria-label="${label} name" placeholder="Name" autocomplete="off" spellcheck="false" value="${escapeHtml(pair.key)}">
        <input data-pair-value aria-label="${label} value" placeholder="Value" autocomplete="off" spellcheck="false" value="${escapeHtml(pair.value)}">
        <button type="button" data-remove-pair="${kind}" data-pair-index="${index}" aria-label="Remove ${label.toLowerCase()} row">${icon('trash')}</button>
      </div>`).join('')}
    </div>
    <button class="key-value-add" type="button" data-add-pair="${kind}">+ Add ${kind === 'query' ? 'parameter' : 'header'}</button>
  </fieldset>`;
}

function operationWorkbenchPanel(operationId) {
  const operation = serviceData.operations.find((candidate) => candidate.id === operationId);
  const draft = workbenchDraft(operationId);
  const result = state.workbenchResult?.operationId === operationId ? state.workbenchResult : null;
  const writable = Boolean(runtime.control?.writable);
  const scenarioOptions = serviceData.scenarios.map((scenario) => `<option value="${escapeHtml(scenario.name)}" ${draft.scenario === scenario.name ? 'selected' : ''}>${escapeHtml(scenario.name)}</option>`).join('');
  const faultOptions = serviceData.faultProfiles.map((profile) => `<option value="${escapeHtml(profile.name)}" ${draft.faultProfile === profile.name ? 'selected' : ''}>${escapeHtml(profile.name)}</option>`).join('');
  const profileOptions = activeProfiles.map((profile) => `<option value="${escapeHtml(profile.name)}" ${draft.profile === profile.name ? 'selected' : ''}>${escapeHtml(profile.name)} · ${escapeHtml(profile.principal)}</option>`).join('');
  const response = result ? JSON.stringify(result.response.body ?? null, null, 2) : '';
  const proof = result?.exchange?.proof;
  return `<section class="workbench">
    <header class="workbench__header"><div><h2>Request workbench</h2><p>Run the compiled operation through the same listener, validation, state, and effect pipeline as an integration client.</p></div>${badge(writable ? 'CONTROL READY' : 'READ ONLY', writable ? 'success' : 'warning')}</header>
    <div class="workbench__grid">
      <form class="request-composer" data-workbench-form>
        <div class="request-line"><span>${methodBadge(operation.method)}</span><input name="path" value="${escapeHtml(draft.path)}" aria-label="Request path"></div>
        <div class="composer-fields">
          ${workbenchPairEditor('Headers', 'headers', draft.headers)}
          ${workbenchPairEditor('Query parameters', 'query', draft.query)}
          <label class="composer-body"><span>JSON body <small>Tab inserts two spaces</small></span><textarea class="json-editor" name="body" data-code-editor spellcheck="false" autocapitalize="off" autocomplete="off">${escapeHtml(draft.body)}</textarea></label>
        </div>
        <div class="composer-controls">
          <label><span>Profile</span><select name="profile"><option value="">Request headers</option>${profileOptions}</select></label>
          <label><span>Scenario</span><select name="scenario"><option value="">Default</option>${scenarioOptions}</select></label>
          <label><span>Fault profile</span><select name="faultProfile"><option value="">None</option>${faultOptions}</select></label>
          <label><span>Seed</span><input name="seed" inputmode="numeric" value="${escapeHtml(draft.seed)}"></label>
          <button class="button button--primary" type="button" data-action="run-operation" ${writable ? '' : 'disabled'}>${icon('terminal')}Run request</button>
        </div>
      </form>
      <section class="workbench-response ${result ? 'has-result' : ''}">
        <header><div><h3>Response</h3><p>${result ? `Captured as ${result.exchange?.id ?? 'an unrecorded response'}` : 'Run the request to inspect its actual response and proof.'}</p></div>${result ? `<span class="status-code ${Number(result.response.status) < 400 ? 'status-code--good' : 'status-code--warn'}">${result.response.status}</span>` : ''}</header>
        ${result ? `<dl class="payload-facts"><div><dt>Status</dt><dd><code>${result.response.status}</code></dd></div><div><dt>Content type</dt><dd><code>${escapeHtml(result.response.contentType || '—')}</code></dd></div><div><dt>Outcome</dt><dd><code>${escapeHtml(result.exchange?.outcome || '—')}</code></dd></div><div><dt>Duration</dt><dd><code>${result.exchange ? formatDuration(result.exchange.durationMicros) : '—'}</code></dd></div></dl>${codePanel('Response body', response, {copy:'workbench response',className:'code-panel--large'})}${proof ? executionProofPanel(proof, true) : ''}` : `<div class="workbench-empty">${icon('operations')}<strong>No request has run yet</strong><span>The result will remain tied to this operation while you inspect it.</span></div>`}
      </section>
    </div>
  </section>`;
}

function readWorkbenchPairs(form, kind) {
  return [...form.querySelectorAll(`[data-pair-row="${kind}"]`)].map((row) => ({
    key: row.querySelector('[data-pair-key]')?.value.trim() ?? '',
    value: row.querySelector('[data-pair-value]')?.value ?? '',
  }));
}

function collectWorkbenchDraft(operation, form) {
  const values = new FormData(form);
  return {
    path: String(values.get('path') || operation.path),
    query: readWorkbenchPairs(form, 'query'),
    headers: readWorkbenchPairs(form, 'headers'),
    body: String(values.get('body') || ''),
    scenario: String(values.get('scenario') || ''),
    faultProfile: String(values.get('faultProfile') || ''),
    profile: String(values.get('profile') || ''),
    seed: String(values.get('seed') || ''),
  };
}

function repeatedWorkbenchValues(pairs) {
  return pairs.reduce((values, pair) => {
    if (!pair.key) return values;
    (values[pair.key] ??= []).push(pair.value);
    return values;
  }, {});
}

function updateWorkbenchPairs(kind, index = null) {
  const operationId = route().detail;
  const operation = serviceData.operations.find((candidate) => candidate.id === operationId);
  const form = document.querySelector('[data-workbench-form]');
  if (!operation || !form) return;
  const draft = collectWorkbenchDraft(operation, form);
  if (index == null) draft[kind].push({ key: '', value: '' });
  else draft[kind].splice(index, 1);
  state.workbenchDrafts[operationId] = draft;
  renderShell();
  const rows = document.querySelectorAll(`[data-pair-row="${kind}"]`);
  rows[Math.min(index ?? rows.length - 1, rows.length - 1)]?.querySelector('[data-pair-key]')?.focus({ preventScroll: true });
}

function handleCodeEditorKeydown(event) {
  if (event.key !== 'Tab') return;
  event.preventDefault();
  const editor = event.currentTarget;
  const start = editor.selectionStart;
  const end = editor.selectionEnd;
  if (start === end) {
    if (event.shiftKey) {
      const lineStart = editor.value.lastIndexOf('\n', start - 1) + 1;
      const removable = editor.value.slice(lineStart, start).match(/^ {1,2}/)?.[0].length ?? 0;
      if (removable) editor.setRangeText('', lineStart, lineStart + removable, 'end');
      return;
    }
    editor.setRangeText('  ', start, end, 'end');
    return;
  }
  const lineStart = editor.value.lastIndexOf('\n', start - 1) + 1;
  const lineEnd = editor.value.indexOf('\n', end);
  const blockEnd = lineEnd === -1 ? editor.value.length : lineEnd;
  const block = editor.value.slice(lineStart, blockEnd);
  const indented = event.shiftKey ? block.replace(/^ {1,2}/gm, '') : block.replace(/^/gm, '  ');
  editor.setRangeText(indented, lineStart, blockEnd, 'select');
}

async function runOperationWorkbench() {
  const operationId = route().detail;
  const operation = serviceData.operations.find((candidate) => candidate.id === operationId);
  const form = document.querySelector('[data-workbench-form]');
  if (!operation || !form) return;
  const draft = collectWorkbenchDraft(operation, form);
  state.workbenchDrafts[operationId] = draft;
  let body;
  try {
    body = draft.body.trim() ? JSON.parse(draft.body) : null;
  } catch (error) {
    showToast(`JSON body is invalid: ${error.message}`, 'warning');
    return;
  }
  const unnamed = [...draft.query, ...draft.headers].find((pair) => !pair.key && pair.value);
  if (unnamed) {
    showToast('Every parameter and header value needs a name', 'warning');
    return;
  }
  showToast(`Running ${operationId}…`);
  try {
    const result = await inspectorJson('/.simile/console/v1/execute', {
      method: 'POST',
      body: JSON.stringify({
        method: operation.method,
        path: draft.path,
        query: repeatedWorkbenchValues(draft.query),
        headers: repeatedWorkbenchValues(draft.headers),
        ...(body == null ? {} : { body, contentType: 'application/json' }),
        ...(draft.scenario ? { scenario: draft.scenario } : {}),
        ...(draft.faultProfile ? { faultProfile: draft.faultProfile } : {}),
        ...(draft.profile ? { profile: draft.profile } : {}),
        ...(draft.seed ? { responseSeed: Number(draft.seed) } : {}),
      }),
    });
    state.workbenchResult = { operationId, ...result };
    await refreshRuntime(true);
    state.operationTab = 'try';
    renderShell();
    showToast(`${operationId} returned ${result.response.status}`, Number(result.response.status) < 400 ? 'success' : 'warning');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

function operationOutcomesPanel(operationId, outcomes) {
  const rows = outcomes.map((outcome, index) => {
    const key = `${operationId}:${index}`;
    const expanded = state.selectedOutcome === key;
    return `<tr class="outcome-summary-row ${expanded ? 'is-expanded' : ''}" data-outcome="${key}" tabindex="0" aria-expanded="${expanded}" aria-controls="outcome-detail-${index}">
      <td><span class="status-code ${Number(outcome.status) < 400 ? 'status-code--good' : 'status-code--warn'}">${outcome.status}</span></td>
      <td><code>${escapeHtml(outcome.name)}</code></td>
      <td>${escapeHtml(outcome.source)}</td>
      <td><code>${escapeHtml(outcome.responseSchema)}</code></td>
      <td class="outcome-row-action">${icon('chevron')}</td>
    </tr>
    <tr class="outcome-detail-row" id="outcome-detail-${index}" ${expanded ? '' : 'hidden'}><td colspan="5">${outcomeDetail(outcome)}</td></tr>`;
  }).join('');
  return `<section class="table-panel outcome-panel"><div class="panel-heading panel-heading--table"><div><h2>Declared outcomes</h2><p>Select an outcome to inspect its response contract and synthesis fidelity.</p></div></div><table class="data-table outcomes-table">${tableHead(['Status','Outcome','Selection','Response schema',''])}<tbody>${rows}</tbody></table></section>`;
}

function outcomeDetail(outcome) {
  const headers = outcome.headers?.length ? outcome.headers.map((header) => `<code>${escapeHtml(header)}</code>`).join(' ') : 'None declared';
  const fidelity = outcome.provenance === 'schema_synthesized'
    ? 'Simile synthesizes this response from the public schema. It is executable, but no provider-reviewed example selects this exact outcome.'
    : outcome.provenance === 'provider_authored'
      ? 'This outcome is provider-authored and selected through an explicit behavior or scenario.'
      : 'This response is derived from a reviewed OpenAPI example.';
  const body = outcome.example == null
    ? `<div class="outcome-empty-body">No response body is declared for this outcome.</div>`
    : codePanel('Representative response body', outcome.example, { copy: `${outcome.name} response`, className: 'outcome-response-code' });
  return `<section class="outcome-detail">
    <div class="outcome-detail__contract">
      <div><span class="section-label">Contract description</span><p>${escapeHtml(outcome.description || 'No response description is declared.')}</p></div>
      <dl>
        <div><dt>Response schema</dt><dd><code>${escapeHtml(outcome.responseSchema)}</code></dd></div>
        <div><dt>Schema variants</dt><dd>${outcome.responseVariants || 'None'}</dd></div>
        <div><dt>Content type</dt><dd><code>${escapeHtml(outcome.contentType)}</code></dd></div>
        <div><dt>Headers</dt><dd>${headers}</dd></div>
        <div><dt>Selection</dt><dd>${escapeHtml(outcome.source)}</dd></div>
      </dl>
      <p class="outcome-fidelity-note">${fidelity}</p>
    </div>
    ${body}
  </section>`;
}

function operationExchangesPanel(operationId) {
  const rows = displayedExchangeRows().filter(([, , , operation]) => operation === operationId);
  if (!rows.length) return `<section class="empty-state">${icon('exchanges')}<h2>No recorded exchanges</h2><p>This operation has no retained exchange history.</p></section>`;
  return `<section class="table-panel"><div class="panel-heading panel-heading--table"><div><h2>Recent exchanges</h2><p>Requests handled by <code>${operationId}</code>.</p></div><a class="text-link" href="/exchanges">Open exchange workspace</a></div>${exchangeTable(rows, true, true)}</section>`;
}

function operationScenariosPanel(operationId) {
  const outcomeNames = new Set(serviceData.operations.find((operation) => operation.id === operationId)?.outcomes.map((outcome) => outcome.name) ?? []);
  const scenarios = Object.entries(scenarioCatalog).filter(([name]) => outcomeNames.has(name));
  if (!scenarios.length) return `<section class="empty-state">${icon('scenarios')}<h2>No scenarios</h2><p>This operation has no deterministic scenario overrides.</p></section>`;
  return `<section class="table-panel"><div class="panel-heading panel-heading--table"><div><h2>Scenarios</h2><p>Deterministic outcomes declared for <code>${operationId}</code>.</p></div><a class="text-link" href="/scenarios">Open scenario workspace</a></div><table class="data-table">${tableHead(['Scenario', 'Selections', 'Faults', 'Behavior'])}<tbody>${scenarios.map(([id, scenario]) => `<tr><td><code>${id}</code></td><td>${scenario.selections}</td><td>${scenario.faults}</td><td>${scenario.description}</td></tr>`).join('')}</tbody></table></section>`;
}

function exchangesPage() {
  const rows = displayedExchangeRows();
  const successful = rows.filter(([, , , , status]) => Number(status) < 400).length;
  const issues = rows.length - successful;
  const latest = rows[0];
  const sourceLabel = runtime.connected ? 'Live' : 'Preview';
  const emptyLatest = ['—', '—'];
  return `<div class="page exchange-index">
    <header class="page-header exchange-index__header">
      <div>
        <div class="exchange-title"><h1>Exchanges</h1><span class="live-pill">${sourceLabel}</span></div>
        <p>${runtime.connected ? `Requests captured by ${serviceName} on this machine.` : 'Representative request traces remain visible while the local runtime is disconnected.'}</p>
      </div>
      <button class="button button--quiet-danger" type="button" data-action="clear-exchanges" aria-label="Clear exchange history" ${runtime.connected ? '' : 'disabled'}>${icon('trash')}<span>Clear history</span></button>
    </header>
    <dl class="exchange-overview" aria-label="Exchange summary">
      <div><dt>Runtime data</dt><dd>${runtime.connected ? 'Connected' : 'Not connected'}</dd></div>
      <div><dt>Displayed</dt><dd>${rows.length} ${runtime.connected ? 'captured' : 'samples'}</dd></div>
      <div><dt>Latest response</dt><dd><span class="status-code ${latest && Number(latest[4]) < 400 ? 'status-code--good' : 'status-code--warn'}">${latest?.[4] ?? emptyLatest[0]}</span><span>${latest?.[5] ?? emptyLatest[1]}</span></dd></div>
      <div><dt>Profile</dt><dd><code>${activeProfileName}</code></dd></div>
    </dl>
    <section class="exchange-ledger" aria-label="Exchange history">
      <header class="exchange-controls">
        <label class="search-field exchange-search">${icon('search')}<input type="search" value="${escapeHtml(state.exchangeQuery)}" placeholder="Search requests, paths, or operations" aria-label="Search exchanges" data-exchange-search><kbd>/</kbd></label>
        <div class="exchange-controls__end">
          <div class="segmented-filter" aria-label="Filter exchanges by response status">
            <button class="${state.exchangeFilter === 'all' ? 'is-active' : ''}" type="button" data-exchange-filter="all" aria-pressed="${state.exchangeFilter === 'all'}">All <span>${rows.length}</span></button>
            <button class="${state.exchangeFilter === 'success' ? 'is-active' : ''}" type="button" data-exchange-filter="success" aria-pressed="${state.exchangeFilter === 'success'}">Successful <span>${successful}</span></button>
            <button class="${state.exchangeFilter === 'issues' ? 'is-active' : ''}" type="button" data-exchange-filter="issues" aria-pressed="${state.exchangeFilter === 'issues'}">Issues <span>${issues}</span></button>
          </div>
          <button class="button button--compact exchange-download" type="button" data-action="download-exchanges">${icon('download')}Export</button>
        </div>
      </header>
      <div class="exchange-table-scroll">${rows.length ? exchangeTable(rows, true, true) : `<section class="empty-state">${icon('exchanges')}<h2>No captured exchanges</h2><p>Send a request to <code>${localServiceUrl}</code> and it will appear here.</p></section>`}</div>
      <footer class="table-footer exchange-footer"><span data-exchange-result-count>Showing all ${rows.length} ${runtime.connected ? 'captured' : 'representative'} exchanges</span><div><button disabled>Previous</button><button disabled>Next</button></div></footer>
    </section>
  </div>`;
}

function exchangeTable(rows, headerVisible, modern = false) {
  if (modern) {
    return `<table class="data-table exchange-table exchange-table--modern">${tableHead(['Received', 'Request', 'Operation', 'Response', 'Latency', ''])}<tbody>${rows.map(([id,method,path,operation,status,duration,result,time]) => {
      const statusGroup = Number(status) < 400 ? 'success' : 'issues';
      const tone = statusGroup === 'success' ? 'success' : 'warning';
      return `<tr class="clickable-row" data-exchange="${id}" data-exchange-status="${statusGroup}" tabindex="0" aria-label="Open ${method} ${path} exchange">
        <td><span class="exchange-time">${time}</span><small>${id.slice(0, 13)}…</small></td>
        <td><span class="exchange-request">${methodBadge(method)}<code>${path}</code></span></td>
        <td><strong>${operation}</strong></td>
        <td><span class="exchange-response"><span class="status-code ${statusGroup === 'success' ? 'status-code--good' : 'status-code--warn'}">${status}</span>${badge(result, tone)}</span></td>
        <td class="mono">${duration}</td>
        <td class="exchange-row-action">${icon('chevron')}</td>
      </tr>`;
    }).join('')}</tbody></table>`;
  }
  return `<table class="data-table exchange-table">${headerVisible ? tableHead(['Time', 'Method', 'Path', 'Operation', 'Status', 'Duration', 'Result']) : ''}<tbody>${rows.map(([id,method,path,operation,status,duration,result,time]) => `<tr class="clickable-row" data-exchange="${id}" tabindex="0"><td class="mono subtle">${time}</td><td>${methodBadge(method)}</td><td><code>${path}</code></td><td><strong>${operation}</strong></td><td><span class="status-code ${Number(status) < 400 ? 'status-code--good' : 'status-code--warn'}">${status}</span></td><td class="mono">${duration}</td><td>${badge(result, 'success')}</td></tr>`).join('')}</tbody></table>`;
}

function liveLogEntries() {
  if (!runtime.connected) return [];
  const http = runtime.exchanges.map((exchange) => ({
    id: `http:${exchange.id}`,
    at: Number(exchange.startedAtMs),
    level: Number(exchange.status) >= 500 ? 'error' : Number(exchange.status) >= 400 ? 'warn' : 'info',
    source: 'HTTP',
    message: `${exchange.method} ${exchange.path} → ${exchange.status}`,
    detail: `${operationIdByKey.get(exchange.operation) || exchange.operation} · ${formatDuration(exchange.durationMicros)}`,
  }));
  const stack = (runtime.inspectionState?.events ?? []).filter((event) => {
    // Why: the service log should show customer-facing API activity, not Simile's own profile and
    // delivery bookkeeping, which is implementation noise and can overwhelm useful events.
    return !String(event.resource?.name || '').startsWith('__simile_');
  }).map((event) => {
    const status = event.outcome?.status ?? 'succeeded';
    return {
      id: `stack:${event.sequence}`,
      at: Number(event.at),
      level: status === 'failed' ? 'error' : 'info',
      source: 'STACK',
      message: `${event.resource?.kind || 'stack'}.${event.resource?.name || 'resource'} · ${event.action || 'activity'}`,
      detail: `${status}${event.operationId ? ` · ${event.operationId}` : ''}`,
    };
  });
  return [...http, ...stack].sort((left, right) => left.at - right.at);
}

function logTimestamp(milliseconds) {
  return new Intl.DateTimeFormat('sv-SE', {
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false,
  }).format(new Date(milliseconds));
}

function logDock() {
  const normalizedQuery = state.logQuery.trim().toLowerCase();
  const entries = liveLogEntries();
  const visibleCount = entries.filter((entry) => !normalizedQuery
    || `${entry.level} ${entry.source} ${entry.message} ${entry.detail}`.toLowerCase().includes(normalizedQuery)).length;
  const expanded = state.logDock !== 'collapsed';
  const full = state.logDock === 'full';
  const rows = entries.map((entry) => {
    const symbol = entry.level === 'error' ? '×' : entry.level === 'warn' ? '!' : '✓';
    return `<li class="log-entry log-entry--${entry.level}">
      <span class="log-entry__rail" aria-hidden="true"></span>
      <time>${logTimestamp(entry.at)}</time>
      <span class="log-entry__status" aria-label="${entry.level}">${symbol}</span>
      <code class="log-entry__source">${entry.source}</code>
      <code class="log-entry__message">${escapeHtml(entry.message)}</code>
      <span class="log-entry__detail">${escapeHtml(entry.detail)}</span>
    </li>`;
  }).join('');
  return `<section class="log-dock" aria-label="Live logs">
    <header class="log-dock__header">
      <button class="log-dock__title" type="button" data-action="toggle-log-dock" aria-expanded="${expanded}">
        ${icon('logs')}<strong>Live logs</strong><span class="log-dock__count" data-log-count>${visibleCount}</span>
        <span class="log-dock__live"><i class="${state.autoRefresh ? 'live-dot' : 'idle-dot'}"></i>${state.autoRefresh ? 'Live' : 'Paused'}</span>
      </button>
      <div class="log-dock__tools">
        ${expanded ? `<label class="log-dock__search">${icon('search')}<input type="search" data-log-search value="${escapeHtml(state.logQuery)}" placeholder="Filter logs" aria-label="Filter live logs"></label>` : ''}
        <button class="log-dock__icon" type="button" data-action="refresh-runtime" aria-label="Refresh live logs" title="Refresh live logs">${icon('replay')}</button>
        <button class="log-dock__icon" type="button" data-action="toggle-log-fullscreen" aria-label="${full ? 'Restore split log view' : 'Show logs full screen'}" title="${full ? 'Restore split view' : 'Full screen'}">${icon(full ? 'restore' : 'maximize')}</button>
        <button class="log-dock__icon log-dock__collapse" type="button" data-action="toggle-log-dock" aria-label="${expanded ? 'Collapse live logs' : 'Expand live logs'}" title="${expanded ? 'Collapse' : 'Expand'}">${icon('collapse')}</button>
      </div>
    </header>
    <div class="log-dock__body" role="log" aria-live="polite" aria-label="Sanitized runtime activity">
      ${rows ? `<ol class="log-stream" data-log-stream>${rows}</ol>` : `<div class="log-dock__empty">${icon('logs')}<span>${runtime.connected ? 'Waiting for HTTP and Stack activity…' : `Waiting for ${escapeHtml(localServiceUrl)}…`}</span></div>`}
    </div>
  </section>`;
}

function statePage() {
  const live = runtime.connected && runtime.inspectionState;
  const source = live ? runtimeResourceGroups() : resourceGroups;
  const groups = Object.entries(source);
  if (!source[state.selectedResourceGroup]) state.selectedResourceGroup = groups[0]?.[0];
  const records = source[state.selectedResourceGroup] || [];
  const selected = records.find((record) => record.id === state.selectedResource) || records[0];
  if (!selected) return `<div class="page">${header('State', 'No Stack resources are declared for this service.')}</div>`;
  return `<div class="page page--flush">${header('State', live ? `Read-only state owned by ${serviceName}.` : 'Representative state shown while the local runtime is disconnected.', `<span class="preview-badge">${live ? 'Live runtime' : 'Preview data'}</span><button class="button" data-action="state-command">${icon('terminal')}Inspection command</button>`)}<div class="split-workbench"><aside class="resource-browser"><label class="search-field">${icon('search')}<input type="search" placeholder="Search resources" aria-label="Search resources"></label><div class="resource-groups">${groups.map(([name,items]) => `<button class="resource-group ${state.selectedResourceGroup === name ? 'is-active' : ''}" data-resource-group="${name}"><span>${icon('state')}<strong>${name}</strong></span><span>${items.length}</span></button>`).join('')}</div></aside><section class="resource-list"><header><div><strong>${state.selectedResourceGroup}</strong><span>${records.length} resource${records.length === 1 ? '' : 's'}</span></div></header>${records.map(({id,status,summary}) => `<button class="resource-row ${selected.id === id ? 'is-active' : ''}" data-resource="${id}"><span><strong>${id}</strong><small>${status}</small></span><span>${summary}</span></button>`).join('')}</section><section class="resource-detail"><header><div>${badge(live ? 'LIVE' : 'PREVIEW',live ? 'success' : 'neutral')}<h2>${selected.id}</h2></div><div><button class="button button--compact" data-action="copy-resource">${icon('copy')}Copy</button></div></header><dl class="detail-facts"><div><dt>Kind</dt><dd>${state.selectedResourceGroup}</dd></div><div><dt>Journal events</dt><dd>${selected.eventCount ?? '—'}</dd></div><div><dt>Last operation</dt><dd><code>${selected.createdBy || '—'}</code></dd></div><div><dt>Source</dt><dd>${live ? 'Running Stack' : selected.updated}</dd></div></dl>${codePanel(live ? 'Current resource state' : 'Representative stored value', selected.body, {copy:'stored resource'})}</section></div></div>`;
}

function runtimeResourceGroups() {
  const events = runtime.inspectionState?.events || [];
  return (runtime.inspectionState?.resources || []).reduce((groups, resource) => {
    const kind = resource.address.kind;
    const id = `${kind}.${resource.address.name}`;
    const related = events.filter((event) => event.resource.kind === kind && event.resource.name === resource.address.name);
    const latest = related.at(-1);
    (groups[kind] ||= []).push({
      id,
      status: resource.value == null ? 'empty' : 'stored',
      summary: `${related.length} event${related.length === 1 ? '' : 's'}`,
      createdBy: latest?.operationId || latest?.action,
      eventCount: related.length,
      updated: 'Live runtime',
      body: JSON.stringify(resource.value ?? null, null, 2),
    });
    return groups;
  }, {});
}

function deferredPage() {
  const deferred = runtime.connected ? runtime.deferred : null;
  const items = deferred?.items ?? [];
  const queueState = items.length ? `${items.length} runtime item${items.length === 1 ? '' : 's'}` : 'No runtime work is queued';
  const runtimeRows = items.map((item) => {
    const work = item.work || {};
    const status = item.queue === 'dead_letter' ? 'FAILED' : item.leased ? 'LEASED' : 'PENDING';
    const tone = item.queue === 'dead_letter' ? 'warning' : item.leased ? 'success' : 'neutral';
    return `<tr><td><code>${item.id}</code></td><td><code>${work.name || '—'}</code></td><td><a class="operation-link" href="/operations/${operationIdByKey.get(work.operation) || work.operation || ''}"><code>${work.operation || '—'}</code></a></td><td>${work.kind || '—'}</td><td>${item.receiveCount}</td><td>${badge(status, tone)}</td></tr>`;
  }).join('');
  const declaredRows = serviceData.asyncOperations.map((operation) => `<tr><td><code>—</code></td><td><code>${operation.name}</code></td><td><code>${operation.name}</code></td><td>${operation.description}</td><td>0</td><td>${badge('DECLARED')}</td></tr>`).join('');
  return `<div class="page">${header('Deferred work', `Asynchronous actions declared by ${serviceName}`, `<button class="button" data-action="refresh-runtime">Refresh</button>`)}<div class="status-strip status-strip--compact"><div>${badge(items.length ? 'ACTIVE' : 'IDLE', items.length ? 'success' : 'neutral')}<strong>${queueState}</strong></div><dl><div><dt>Declared</dt><dd>${serviceData.asyncOperations.length}</dd></div><div><dt>Pending</dt><dd>${deferred?.pending ?? 0}</dd></div><div><dt>Leased</dt><dd>${deferred?.leased ?? 0}</dd></div><div><dt>Failed</dt><dd>${deferred?.deadLetters ?? 0}</dd></div></dl></div>${toolbar({search:'Search asynchronous work',count:deferred ? `${items.length} runtime items` : `${serviceData.asyncOperations.length} declared operations`,actions:`<button class="button button--compact" data-action="filter-deferred">${icon('filter')}Filter</button>`})}<section class="table-panel"><table class="data-table">${tableHead(['Message', 'Name', 'Operation', 'Kind', 'Attempts', 'Runtime state'])}<tbody>${deferred ? runtimeRows : declaredRows}</tbody></table></section></div>`;
}

function scenariosPage() {
  const selected = scenarioCatalog[state.selectedScenario];
  if (!selected) {
    return `<div class="page">${header('Scenarios', 'No deterministic scenarios are declared for this service.')}<section class="empty-state">${icon('scenarios')}<h2>No scenarios</h2><p>Add a scenario to the Simfile when a test needs a named, repeatable outcome.</p></section></div>`;
  }
  const enabled = !state.disabledScenarios.has(state.selectedScenario);
  const matchingOperations = serviceData.operations.filter((operation) => operation.outcomes.some((outcome) => outcome.name === state.selectedScenario));
  return `<div class="page page--flush">${header('Scenarios', `${serviceData.scenarios.length} public, deterministic test scenarios compiled for ${serviceName}`, `<button class="button button--primary" data-action="create-scenario">New scenario</button>`)}<div class="scenario-layout"><aside class="scenario-list"><label class="search-field">${icon('search')}<input type="search" placeholder="Search scenarios" aria-label="Search scenarios"></label>${Object.entries(scenarioCatalog).map(([id,scenario]) => `<button class="scenario-row ${state.selectedScenario === id ? 'is-active' : ''}" data-scenario="${id}"><span><strong>${scenario.name}</strong><small>${scenario.selections} selection${scenario.selections === 1 ? '' : 's'} · ${scenario.faults} fault${scenario.faults === 1 ? '' : 's'}</small></span>${icon('chevron')}</button>`).join('')}</aside><section class="scenario-detail"><header><div>${badge(enabled ? 'ACTIVE' : 'DISABLED',enabled ? 'success' : 'neutral')}<h2>${selected.name}</h2><p>${selected.description}</p></div><button class="button" data-action="toggle-scenario">${enabled ? 'Disable' : 'Enable'}</button></header><dl class="detail-facts detail-facts--grid"><div><dt>Scenario ID</dt><dd><code>${state.selectedScenario}</code></dd></div><div><dt>Outcome selections</dt><dd>${selected.selections}</dd></div><div><dt>Fault injections</dt><dd>${selected.faults}</dd></div><div><dt>Matching operations</dt><dd>${matchingOperations.length}</dd></div></dl><div class="scenario-rule"><span class="rule-index">WHEN</span><code>X-Simile-Scenario: ${state.selectedScenario}</code></div><div class="scenario-rule"><span class="rule-index">THEN</span><code>activate compiled scenario selections and faults</code></div><section class="table-panel scenario-operations"><div class="panel-heading panel-heading--table"><div><h3>Resolved operations</h3><p>Operations with a named outcome matching this scenario.</p></div></div>${matchingOperations.length ? `<table class="data-table">${tableHead(['Operation', 'Method', 'Path'])}<tbody>${matchingOperations.map((operation) => `<tr><td><a class="operation-link" href="/operations/${operation.id}"><code>${operation.id}</code></a></td><td>${methodBadge(operation.method)}</td><td><code>${operation.path}</code></td></tr>`).join('')}</tbody></table>` : `<div class="stack-empty">This scenario applies a transport fault or a selection whose operation mapping is not exposed by the current describe API.</div>`}</section></section></div></div>`;
}

function faultsPage() {
  const profile = serviceData.faultProfiles.find((candidate) => candidate.name === state.selectedFaultProfile);
  if (!profile) {
    return `<div class="page">${header('Faults', 'No fault profile is active for this service.')}<section class="empty-state">${icon('faults')}<h2>No fault profiles</h2><p>Fault profiles make bounded transport failures available to tests.</p></section></div>`;
  }
  const enabled = new Set(profile?.faults ?? []);
  return `<div class="page">${header('Faults', `${serviceData.faults.length} transport faults and ${serviceData.faultProfiles.length} bounded profiles compiled for ${serviceName}`, `<button class="button button--primary" data-action="new-fault">New fault profile</button>`)}<div class="profile-tabs">${serviceData.faultProfiles.map((candidate) => `<button class="${state.selectedFaultProfile === candidate.name ? 'is-active' : ''}" data-fault-profile="${candidate.name}">${state.selectedFaultProfile === candidate.name ? '<span class="live-dot"></span>' : ''}${candidate.name}</button>`).join('')}</div><section class="fault-grid">${serviceData.faults.map((fault) => `<div class="panel"><div class="panel-heading"><div><h2>${fault.name}</h2><p>${fault.description}</p></div>${badge(enabled.has(fault.name) ? 'ACTIVE' : 'AVAILABLE', enabled.has(fault.name) ? 'success' : 'neutral')}</div><dl class="fault-facts"><div><dt>Phase</dt><dd><code>${fault.phase}</code></dd></div><div><dt>Action</dt><dd><code>${fault.action}</code></dd></div></dl></div>`).join('')}</section><div class="notice-panel">${icon('check')}<div><strong>${profile.name} · ${(profile.probabilityPpm / 10000).toFixed(2)}% probability</strong><p>${profile.description} ${profile.operations.length ? `${profile.operations.length} operations are in scope.` : 'All matching operations are in scope.'}</p></div></div><section class="table-panel"><div class="panel-heading panel-heading--table"><div><h2>Profile operation scope</h2><p>Exact structural operation keys affected by this profile.</p></div></div>${profile.operations.length ? `<table class="data-table">${tableHead(['Operation'])}<tbody>${profile.operations.map((operation) => `<tr><td><code>${operation}</code></td></tr>`).join('')}</tbody></table>` : `<div class="stack-empty">This profile applies across the public service.</div>`}</section></div>`;
}

function fidelityPage() {
  const metrics = serviceData.metrics;
  const exact = metrics.outcomes - metrics.schemaSynthesizedOutcomes;
  return `<div class="page">${header('Fidelity', `What ${serviceName} can reproduce exactly and where it still approximates`, `<button class="button" data-action="download-report">${icon('download')}Download report</button>`)}<div class="fidelity-hero"><div class="score-ring"><svg viewBox="0 0 120 120"><circle class="score-track" cx="60" cy="60" r="49"/><circle class="score-value" cx="60" cy="60" r="49"/></svg><div><strong>${metrics.errors}</strong><span>audit errors</span></div></div><div class="score-summary"><h2>${metrics.operations} operations compile; ${metrics.warnings} gaps remain visible</h2><p>The service is executable and public-export-safe. Schema synthesis remains the dominant approximation, so the audit does not turn a valid build into a false fidelity score.</p><div><span>${badge(`${exact} REVIEWED`,'success')}</span><span>${badge(`${metrics.schemaSynthesizedOutcomes} SYNTHESIZED`)}</span><span>${badge(`${metrics.warnings} WARNINGS`,'warning')}</span></div></div></div><div class="quality-grid"><div><strong>${metrics.operations} / ${metrics.operations}</strong><span>Operations executable</span></div><div><strong>${metrics.providerAuthoredOutcomes}</strong><span>Provider-authored outcomes</span></div><div><strong>${metrics.underspecifiedOperations}</strong><span>Schema-only operations</span></div><div><strong>${serviceData.artifact.publicExportSafe ? 'Yes' : 'No'}</strong><span>Public export safe</span></div></div><section class="table-panel"><div class="panel-heading panel-heading--table"><div><h2>Audit findings</h2><p>Showing the first 30 of ${metrics.warnings} current findings.</p></div></div><table class="data-table">${tableHead(['Severity','Code','Subject','Finding'])}<tbody>${fidelityFindingRows(30)}</tbody></table><footer class="table-footer"><span>${metrics.errors} errors · ${metrics.warnings} warnings</span></footer></section></div>`;
}

function settingsPage() {
  const selected = configurationFiles.find((file) => file.id === state.selectedConfigFile) ?? configurationFiles[0];
  const configuration = selected ? `<section class="settings-section settings-configuration">
    <div class="settings-section-heading"><div><h2>Service configuration</h2><p>Exact source snapshot used to generate ${serviceName}.</p></div>${badge('SOURCE')}</div>
    <div class="config-workbench">
      <div class="config-file-tabs" role="tablist" aria-label="Service configuration files">${configurationFiles.map((file) => `<button role="tab" aria-selected="${file.id === selected.id}" class="${file.id === selected.id ? 'is-active' : ''}" data-config-file="${file.id}">${file.name}</button>`).join('')}</div>
      <div class="config-source code-panel" role="tabpanel">
        <header><code>${selected.path}</code><button class="icon-only" data-copy="${selected.name}" aria-label="Copy ${selected.name}">${icon('copy')}</button></header>
        <pre><code>${syntaxHcl(selected.contents)}</code></pre>
      </div>
    </div>
  </section>` : '';
  return `<div class="page page--narrow">${header('Settings', 'Service configuration and local console preferences')}
    <section class="settings-section"><h2>Service configuration</h2><div class="settings-row"><div><strong>Registered service</strong><p>Select any service registered on this machine.</p></div><button class="select-button" data-menu="service-switcher">${serviceName}<span class="down-caret"></span></button></div><div class="settings-row"><div><strong>Listener</strong><p>Address used by integration clients.</p></div><code>${localServiceUrl}</code></div><div class="settings-row"><div><strong>Active profile</strong><p>Consumer setup applied to requests.</p></div><button class="select-button" data-action="choose-profile">${activeProfileName}<span class="down-caret"></span></button></div><div class="settings-row"><div><strong>Contract source</strong><p>Compiled public contract for this local service.</p></div><code>${serviceData.service.contract}</code></div><div class="settings-row"><div><strong>Simfile</strong><p>Composition, behavior, state, profiles, and faults.</p></div><code>${serviceData.service.source}</code></div></section>
    ${configuration}
    <section class="settings-section"><h2>Interface</h2><div class="settings-row"><div><strong>Auto-refresh runtime</strong><p>Keep exchanges, logs, state, and deferred work current.</p></div><label class="switch"><input type="checkbox" data-action="auto-refresh" ${state.autoRefresh ? 'checked' : ''}><span></span></label></div><div class="settings-row"><div><strong>Issue sounds</strong><p>Play a short alert for a new 4xx/5xx exchange or runtime disconnect.</p></div><span class="settings-actions"><button class="button button--compact" data-action="test-sound" ${state.soundEnabled ? '' : 'disabled'}>Test sound</button><label class="switch"><input type="checkbox" data-action="toggle-sound" ${state.soundEnabled ? 'checked' : ''}><span></span></label></span></div><div class="settings-row"><div><strong>Expand JSON by default</strong><p>Open structured payloads when selecting events.</p></div><label class="switch"><input type="checkbox" checked><span></span></label></div><div class="settings-row"><div><strong>Dense tables</strong><p>Reduce row height for large contracts.</p></div><label class="switch"><input type="checkbox" checked><span></span></label></div></section><section class="settings-section settings-section--danger"><h2>Local data</h2><div class="settings-row"><div><strong>Reset service state</strong><p>Clears every resource and journal entry in the running local service.</p></div><button class="button button--danger" data-action="reset-state" ${runtime.connected ? '' : 'disabled'}>Reset local data</button></div></section></div>`;
}

function docsPage() {
  return `<div class="page page--narrow">${header('Documentation', 'Use Simile locally and understand what the emulator guarantees')}<section class="docs-list"><a href="https://os.inttegro.dev/simile/quickstart" target="_blank" rel="noreferrer"><span>${icon('docs')}<span><strong>Quickstart</strong><small>Install Simile and run your first service.</small></span></span>${icon('chevron')}</a><a href="https://os.inttegro.dev/simile/service-configuration" target="_blank" rel="noreferrer"><span>${icon('settings')}<span><strong>Service configuration</strong><small>Compose contracts, behaviors, listeners, and profiles.</small></span></span>${icon('chevron')}</a><a href="https://os.inttegro.dev/simile/state-and-replay" target="_blank" rel="noreferrer"><span>${icon('state')}<span><strong>State and replay</strong><small>Understand persisted resources and causal history.</small></span></span>${icon('chevron')}</a><a href="https://os.inttegro.dev/simile/outcome-fidelity" target="_blank" rel="noreferrer"><span>${icon('fidelity')}<span><strong>Outcome fidelity</strong><small>Measure exact outcomes and schema synthesis gaps.</small></span></span>${icon('chevron')}</a></section><section class="command-panel"><div><span class="terminal-dots"><i></i><i></i><i></i></span><strong>Open this service from the terminal</strong></div><code>simile console ${serviceName}</code><button class="icon-only" data-copy="terminal command" aria-label="Copy terminal command">${icon('copy')}</button></section></div>`;
}

function exchangeDetail() {
  const liveExchange = runtime.exchanges.find((exchange) => exchange.id === route().detail);
  if (liveExchange) return liveExchangeDetail(liveExchange);
  const tabs = [['timeline','Timeline'],['request','Request'],['response','Response'],['state','State 1'],['deferred','Deferred work 0'],['faults','Faults 0'],['validation','Validation']];
  const tabLabels = tabs.map(([key, label]) => [key, label.replace(/ (\d+)$/, ' <span class="tab-count">$1</span>')]);
  return `<div class="page exchange-page"><a class="back-link" href="/exchanges">‹ Back to Exchanges</a><div class="preview-notice"><span>Preview trace</span>This representative exchange demonstrates the intended inspection workflow; it was not streamed from the current runtime.</div><div class="exchange-heading"><div><div class="title-line">${badge('DELIVERED','success')}${badge('PREVIEW','neutral')}<h1><a class="operation-link" href="/operations/${representativeOperation.id}">${representativeOperation.id}</a><span aria-hidden="true">·</span><code>${exchangeId}</code></h1></div><dl class="exchange-summary"><div><dt>Started</dt><dd>Nov 22, 2026, 7:18:04.114 PM</dd></div><div><dt>Request ID</dt><dd><code>req_01JDC6Y8P7Y4K0FJ9Q3T5</code></dd></div><div><dt>Response</dt><dd><code>${representativeOperation.success} · ${representativeResponseSize}</code></dd></div><div><dt>Ended</dt><dd>Nov 22, 2026, 7:18:04.152 PM</dd></div><div><dt>Operation</dt><dd><a class="operation-link" href="/operations/${representativeOperation.id}"><code>${representativeOperation.key} · ${representativeOperation.id}</code></a></dd></div><div><dt>Outcome</dt><dd><code>${representativeOutcomeName}</code></dd></div><div><dt>Duration</dt><dd><code>38ms</code></dd></div><div><dt>Profile</dt><dd><code>${activeProfileName}</code></dd></div><div><dt>Contract</dt><dd><span class="source-chip">OAS</span>${serviceData.service.version}</dd></div></dl></div><div class="page-actions"><button class="button" data-action="replay" disabled>${icon('replay')}Replay</button><button class="button exchange-actions-button" data-menu="exchange-actions"><span>More actions</span><span class="down-caret"></span></button></div></div><div class="exchange-tabs">${localTabs(tabLabels, state.exchangeTab, 'exchange-tab', 'Exchange views', 'exchange-tab-panel')}</div><div id="exchange-tab-panel" class="exchange-panel local-tab-panel" role="tabpanel" aria-labelledby="exchange-tab-${state.exchangeTab}-tab">${exchangeTabPanel()}</div></div>`;
}

function exchangeReportPage() {
  const exchange = currentExchange();
  const report = preparedExchangeReport(exchange);
  const verified = Boolean(report.runtimeReport?.verified);
  const submitted = Boolean(report.runtimeReport?.submitted);
  const request = JSON.stringify(report.exchange.request, null, 2);
  const response = JSON.stringify(report.exchange.response, null, 2);
  const journal = JSON.stringify(report.relatedJournalEvents, null, 2);
  const generated = new Date(report.generatedAt).toLocaleString();
  return `<div class="page exchange-page exchange-report-page">
    <a class="back-link" href="/exchanges/${report.exchange.id}">‹ Back to Exchange</a>
    ${header('Exchange report', 'Sanitized technical evidence generated from one captured exchange.', `${verified && !submitted ? '<button class="button button--primary" data-action="submit-exchange-report">Submit report</button>' : ''}<button class="button" data-action="download-exchange-report">${icon('download')}Download JSON</button>`)}
    <div class="report-status-line">${badge(report.source === 'live_inspection' ? 'LIVE' : 'PREVIEW', report.source === 'live_inspection' ? 'success' : 'neutral')}${badge(submitted ? 'SUBMITTED' : verified ? 'VERIFIED' : 'UNVERIFIED', submitted || verified ? 'success' : 'warning')}<code>${report.runtimeReport?.id || report.exchange.id}</code></div>
    <dl class="operation-summary">
      <div><dt>Operation</dt><dd><a class="operation-link" href="/operations/${report.exchange.operation.id}"><code>${report.exchange.operation.id}</code></a></dd></div>
      <div><dt>Request</dt><dd><code>${report.exchange.operation.method} ${report.exchange.operation.path}</code></dd></div>
      <div><dt>Response</dt><dd><code>${report.exchange.status}${report.exchange.outcome ? ` · ${report.exchange.outcome}` : ''}</code></dd></div>
      <div><dt>Duration</dt><dd><code>${formatDuration(report.exchange.durationMicros)}</code></dd></div>
      <div><dt>Generated</dt><dd>${generated}</dd></div>
      <div><dt>Artifact</dt><dd><code>${(report.runtimeReport?.artifactDigest || report.service.artifact.sha256).slice(0, 16)}…</code></dd></div>
    </dl>
    <div class="notice-panel exchange-report-boundary">${icon(verified ? 'check' : 'warning')}<div><strong>${verified ? 'Verification' : 'Evidence boundary'}</strong><p>${report.evidenceBoundary.verification} ${report.evidenceBoundary.submission}</p></div></div>
    <section class="operation-grid exchange-report-evidence">
      ${codePanel('Request evidence', request, {copy:'request evidence',className:'code-panel--large'})}
      ${codePanel('Response evidence', response, {copy:'response evidence',className:'code-panel--large'})}
    </section>
    <section class="exchange-report-journal"><header><div><h2>Related journal events</h2><p>${report.relatedJournalEvents.length ? `${report.relatedJournalEvents.length} state event${report.relatedJournalEvents.length === 1 ? '' : 's'} occurred while this exchange ran.` : 'No journal event falls within this exchange window.'}</p></div></header>${codePanel('Journal evidence', journal, {copy:'journal evidence',className:'code-panel--large'})}</section>
  </div>`;
}

function liveExchangeDetail(exchange) {
  const operationId = operationIdByKey.get(exchange.operation) || exchange.operation;
  const duration = formatDuration(exchange.durationMicros);
  const started = new Date(exchange.startedAtMs);
  const ended = new Date(exchange.startedAtMs + exchange.durationMicros / 1000);
  const responseBody = messageBody(exchange.response);
  const tabs = [['timeline','Timeline'],['request','Request'],['response','Response'],['proof','Why this happened'],['state','State']];
  return `<div class="page exchange-page"><a class="back-link" href="/exchanges">‹ Back to Exchanges</a><div class="exchange-heading"><div><div class="title-line">${badge(Number(exchange.status) < 400 ? 'DELIVERED' : 'ISSUE',Number(exchange.status) < 400 ? 'success' : 'warning')}${badge('LIVE','success')}<h1><a class="operation-link" href="/operations/${operationId}">${operationId}</a><span aria-hidden="true">·</span><code>${exchange.id}</code></h1></div><dl class="exchange-summary"><div><dt>Started</dt><dd>${started.toLocaleString()}</dd></div><div><dt>Request ID</dt><dd><code>${exchange.id}</code></dd></div><div><dt>Response</dt><dd><code>${exchange.status} · ${new TextEncoder().encode(responseBody).length} bytes</code></dd></div><div><dt>Ended</dt><dd>${ended.toLocaleString()}</dd></div><div><dt>Operation</dt><dd><a class="operation-link" href="/operations/${operationId}"><code>${exchange.operation}</code></a></dd></div><div><dt>Outcome</dt><dd><code>${exchange.outcome || '—'}</code></dd></div><div><dt>Duration</dt><dd><code>${duration}</code></dd></div><div><dt>Profile</dt><dd><code>${exchange.profile || 'default'}</code></dd></div><div><dt>Contract</dt><dd><span class="source-chip">OAS</span>${serviceData.service.version}</dd></div></dl></div><div class="page-actions"><button class="button" data-action="replay" ${runtime.control?.writable ? '' : 'disabled'}>${icon('replay')}Replay</button><button class="button exchange-actions-button" data-menu="exchange-actions"><span>More actions</span><span class="down-caret"></span></button></div></div><div class="exchange-tabs">${localTabs(tabs, state.exchangeTab, 'exchange-tab', 'Exchange views', 'exchange-tab-panel')}</div><div id="exchange-tab-panel" class="exchange-panel local-tab-panel" role="tabpanel" aria-labelledby="exchange-tab-${state.exchangeTab}-tab">${liveExchangeTabPanel(exchange)}</div></div>`;
}

function liveExchangeTabPanel(exchange) {
  const requestBody = messageBody(exchange.request);
  const responseBody = messageBody(exchange.response);
  const panels = {
    timeline: () => `<section class="payload-view"><header><div><h2>Runtime exchange</h2><p>The service captured this request and response directly from its loopback listener.</p></div>${badge(formatDuration(exchange.durationMicros),'success')}</header><dl class="payload-facts"><div><dt>Method</dt><dd><code>${exchange.method}</code></dd></div><div><dt>Path</dt><dd><code>${exchange.path}</code></dd></div><div><dt>Status</dt><dd><code>${exchange.status}</code></dd></div><div><dt>Outcome</dt><dd><code>${exchange.outcome || '—'}</code></dd></div></dl><div class="runtime-exchange-flow"><span>Request received</span><i></i><span>Contract executed</span><i></i><span>Response ${exchange.status}</span></div></section>`,
    request: () => payloadPanel('Request', 'Sanitized request payload captured by the local runtime.', [['Method',exchange.method],['Path',`<code>${exchange.path}</code>`],['Content type',exchange.request.contentType || '—'],['Capture',exchange.request.truncated ? 'Body omitted' : 'Complete']], requestBody),
    response: () => payloadPanel('Response', 'Response payload produced by the running service.', [['Status',String(exchange.status)],['Content type',exchange.response.contentType || '—'],['Outcome',exchange.outcome || '—'],['Duration',formatDuration(exchange.durationMicros)]], responseBody),
    proof: () => exchange.proof ? executionProofPanel(exchange.proof, false, state.replayResults[exchange.id]) : `<section class="empty-state">${icon('warning')}<h2>Execution proof unavailable</h2><p>This exchange predates the versioned console protocol.</p></section>`,
    state: () => `<section class="payload-view"><header><div><h2>State after exchange</h2><p>Inspect the current Stack and its payload-free causal journal.</p></div><a class="button button--compact" href="/state">Open live state</a></header></section>`,
  };
  return (panels[state.exchangeTab] || panels.timeline)();
}

function executionProofPanel(proof, compact = false, replay = null) {
  const actions = [
    ...(proof.effects?.synchronous ?? []).map((action) => `${action.resource} · ${action.command}`),
    ...(proof.effects?.asynchronous ?? []).map((operation) => `async · ${operation.name}`),
    ...(proof.effects?.webhooks ?? []).map((name) => `webhook · ${name}`),
  ];
  const replayPanel = replay ? `<section class="replay-result ${replay.matches ? 'is-match' : 'has-diff'}"><header><div>${icon(replay.matches ? 'check' : 'warning')}<span><strong>${replay.matches ? 'Deterministic replay matched' : `${replay.differences.length} semantic difference${replay.differences.length === 1 ? '' : 's'}`}</strong><small>The replay ran without mutating service state.</small></span></div>${badge(replay.matches ? 'MATCH' : 'DIFF', replay.matches ? 'success' : 'warning')}</header>${replay.differences.length ? `<table class="data-table">${tableHead(['Path','Change','Expected','Actual'])}<tbody>${replay.differences.map((difference) => `<tr><td><code>${escapeHtml(difference.path)}</code></td><td>${escapeHtml(difference.kind)}</td><td><code>${escapeHtml(JSON.stringify(difference.expected))}</code></td><td><code>${escapeHtml(JSON.stringify(difference.actual))}</code></td></tr>`).join('')}</tbody></table>` : ''}</section>` : '';
  return `<section class="execution-proof ${compact ? 'execution-proof--compact' : ''}">
    <header><div><span class="eyebrow">Execution proof v${proof.version}</span><h2>Why this response happened</h2><p>${escapeHtml(proof.selection.reason)}</p></div>${badge('PROVEN', 'success')}</header>
    ${replayPanel}
    <div class="proof-grid">
      <section><h3>Selection</h3><dl class="detail-facts"><div><dt>Outcome</dt><dd><code>${escapeHtml(proof.selection.outcome || 'default')}</code></dd></div><div><dt>Source</dt><dd><code>${escapeHtml(proof.selection.source)}</code></dd></div><div><dt>Scenario</dt><dd><code>${escapeHtml(proof.controls.scenario || 'none')}</code></dd></div><div><dt>Response seed</dt><dd><code>${proof.controls.responseSeed}</code></dd></div></dl></section>
      <section><h3>Proof checks</h3><div class="check-list proof-checks">${proof.validation.map((check) => `<div>${icon('check')}<span><strong>${escapeHtml(check.name)}</strong><small>${escapeHtml(check.detail)}</small></span>${badge(check.result.toUpperCase(),'success')}</div>`).join('')}</div></section>
      <section><h3>Consequences</h3>${actions.length ? `<ul class="proof-actions">${actions.map((action) => `<li>${icon('check')}<code>${escapeHtml(action)}</code></li>`).join('')}</ul>` : '<p class="subtle">No state, asynchronous, or webhook effect was planned.</p>'}${proof.fault ? `<div class="proof-fault">${badge('FAULT','warning')}<code>${escapeHtml(proof.fault.name)}</code><span>${escapeHtml(proof.fault.source)}</span></div>` : ''}</section>
    </div>
  </section>`;
}

async function replayCurrentExchange() {
  const exchange = currentExchange();
  if (!runtime.exchanges.some((candidate) => candidate.id === exchange.id)) return;
  showToast(`Replaying ${exchange.id} without changing state…`);
  try {
    const result = await inspectorJson(`/.simile/console/v1/exchanges/${encodeURIComponent(exchange.id)}/replay`, { method: 'POST' });
    state.replayResults[exchange.id] = result;
    state.exchangeTab = 'proof';
    renderShell();
    showToast(result.matches ? 'Replay matched the captured result' : `Replay found ${result.differences.length} differences`, result.matches ? 'success' : 'warning');
  } catch (error) {
    showToast(error.message, 'warning');
  }
}

function exchangeTabPanel() {
  const panels = {
    timeline: timelinePanel,
    request: () => payloadPanel('Request', `Representative request generated from ${representativeOperation.id}.`, [['Method',representativeOperation.method],['Path',representativeOperation.path],['Content type','application/json'],['Schema',representativeOperationDetail.requestSchema]], representativeRequest),
    response: () => payloadPanel('Response', `Representative response generated from ${representativeOperation.id}.`, [['Status',String(representativeOperation.success)],['Content type',representativeOutcome?.contentType ?? 'application/json'],['Size',representativeResponseSize],['Outcome',representativeOutcomeName]], representativeResponse),
    state: stateEffectPanel,
    deferred: () => emptyExchangePanel('deferred', 'No deferred work', 'The selected outcome did not schedule an event, webhook, or asynchronous action.'),
    faults: () => emptyExchangePanel('faults', 'No fault applied', 'This exchange used the default deterministic profile. Transport and dependency behavior were unchanged.'),
    validation: validationPanel,
  };
  return panels[state.exchangeTab]();
}

function payloadPanel(title, description, facts, json) {
  return `<section class="payload-view"><header><div><h2>${title}</h2><p>${description}</p></div><button class="button button--compact" data-copy="${title.toLowerCase()}">${icon('copy')}Copy</button></header><dl class="payload-facts">${facts.map(([label,value]) => `<div><dt>${label}</dt><dd>${value}</dd></div>`).join('')}</dl>${codePanel(`${title} body`, json, {copy:title.toLowerCase(),className:'code-panel--large'})}</section>`;
}

function emptyExchangePanel(iconName, title, copy) {
  return `<section class="empty-state">${icon(iconName)}<h2>${title}</h2><p>${copy}</p></section>`;
}

function stateEffectPanel() {
  return `<section class="payload-view"><header><div><h2>State effect</h2><p>The preview does not invent a state mutation for this operation.</p></div><a class="button button--compact" href="/state">Open state</a></header>${codePanel('Representative response', representativeResponse, {copy:'state effect',className:'code-panel--large'})}</section>`;
}

function validationPanel() {
  const checks = [['Operation matched',`${representativeOperation.key} resolved uniquely to ${representativeOperation.id}.`],['Request schema',`${representativeOperationDetail.requestSchema} accepted the representative body.`],['Response schema',`The ${representativeOperation.success} response matched ${representativeOperationDetail.responseSchema}.`],['Outcome selection',`${representativeOutcomeName} is declared by the compiled operation.`]];
  return `<section class="validation-view"><header>${icon('check')}<div><h2>Contract sound</h2><p>The request, selected outcome, response, and stored state all satisfy the compiled contract.</p></div></header><div class="check-list">${checks.map(([title,copy]) => `<div>${icon('check')}<span><strong>${title}</strong><small>${copy}</small></span>${badge('PASSED','success')}</div>`).join('')}</div></section>`;
}

function timelinePanel() {
  return `<section class="timeline-view"><div class="io-preview-row"><div><span>Request</span><button data-exchange-tab="request"><code>{ "customer_data": { "email_address": "ama@example.test" }, "line_items": [ … ] }</code><span>⌄</span></button></div><div><span>Response</span><button data-exchange-tab="response"><code>{ "order": { "id": "or_simile_requires_payment", "status": "requires_payment", … } }</code><span>⌄</span></button></div></div><div class="timeline-heading"><h2>Timeline <span class="info-mark">i</span></h2><div class="button-group"><button class="button button--compact" data-action="timeline-order">${state.timelineOrder === 'ascending' ? 'Ascending' : 'Descending'}</button><button class="button button--compact is-disabled" disabled>Hide idle time</button><button class="button button--compact" data-action="event-filter">${icon('filter')} ${state.eventFilter === 'all' ? 'Filter' : 'Contract events'}</button><button class="button button--compact" data-action="auto-refresh"><span class="${state.autoRefresh ? 'live-dot' : 'idle-dot'}"></span>Auto refresh ${state.autoRefresh ? 'on' : 'off'}</button><button class="button button--compact" data-action="download-trace">${icon('download')}Download</button></div></div><div class="timeline-frame"><div class="trace-chart"><div class="trace-grid"></div>${[['request','Request','2','10'],['validate','Validate','12','17'],['outcome','Outcome','29','8'],['state','State','37','36'],['response','Synthesize','73','21'],['deliver','200','94','4']].map(([id,label,left,width]) => `<button class="trace-segment trace-segment--${id} ${state.selectedEvent === id ? 'is-selected' : ''}" style="--left:${left}%;--width:${width}%" data-event="${id}"><span>${label}</span></button>`).join('')}<div class="trace-axis"><span>0ms</span><span>5ms</span><span>10ms</span><span>15ms</span><span>20ms</span><span>25ms</span><span>30ms</span><span>35ms</span><span>38ms</span></div></div>${selectedEventPanel()}<div class="history-list ${state.timelineOrder === 'descending' ? 'history-list--reversed' : ''}">${historyRows()}</div></div></section>`;
}

function selectedEventPanel() {
  const events = {
    request:['Request received','4ms',[['Event ID','1'],['Method',representativeOperation.method],['Path',representativeOperation.path],['Principal','application']],'Request',representativeRequest],
    validate:['Request validated','4ms',[['Event ID','2'],['Operation',representativeOperation.id],['Schema',representativeOperationDetail.requestSchema],['Result','Valid']],'Validation','Contract checks passed'],
    outcome:['Outcome selected','2ms',[['Event ID','3'],['Outcome',representativeOutcomeName],['Source',provenanceLabel(representativeOutcome?.provenance ?? 'open_api_derived')],['Selection','Deterministic success']],'Decision','Selected from the operation’s declared outcomes.'],
    state:['State evaluated','17ms',[['Event ID','4'],['Operation',representativeOperation.id],['Mutation','Not asserted by preview']],'State effect','Inspect the live runtime for committed state.'],
    response:['Response synthesized','9ms',[['Event ID','5'],['Operation',representativeOperation.id],['Outcome',representativeOutcomeName],['Status',String(representativeOperation.success)],['Schema',representativeOperationDetail.responseSchema],['Constraints','Contract satisfied']],'Response',representativeResponse],
    deliver:['Response delivered','2ms',[['Event ID','6'],['Status',String(representativeOperation.success)],['Size',representativeResponseSize],['Total duration','38ms']],'Delivery','Completed without a transport fault.'],
  };
  const [title,duration,facts,panelTitle,panelValue] = events[state.selectedEvent];
  const value = panelValue.startsWith('{') ? codePanel(panelTitle,panelValue,{copy:panelTitle,className:'selected-code'}) : `<div class="event-message">${icon('check')}<span>${panelValue}</span></div>`;
  return `<article class="selected-event"><header><div>${badge('COMPLETED','success')}<span class="selected-event__title">${title}</span><code>${duration}</code></div><button type="button" data-action="close-event">Close ×</button></header><div class="selected-event__body"><dl>${facts.map(([label,item]) => `<div><dt>${label}</dt><dd>${label === 'Operation' ? `<a class="operation-link" href="/operations/${item}"><code>${item}</code></a>` : `<code>${item}</code>`}</dd></div>`).join('')}</dl>${value}</div></article>`;
}

function historyRows() {
  const rows = [['request','1','Request received','19:18:04.114',representativeOperation.method,representativeOperation.path],['validate','2','Request validated','19:18:04.118',representativeOperationDetail.requestSchema,'Contract checks passed'],['outcome','3','Outcome selected','19:18:04.122',representativeOutcomeName,provenanceLabel(representativeOutcome?.provenance ?? 'open_api_derived')],['state','4','State evaluated','19:18:04.124',representativeOperation.id,'No preview assertion'],['response','5','Response synthesized','19:18:04.141',String(representativeOperation.success),representativeOperationDetail.responseSchema],['deliver','6','Response delivered','19:18:04.152',representativeResponseSize,'38ms total']];
  return rows.map(([id,index,title,time,left,right]) => `<button class="history-row ${state.selectedEvent === id ? 'is-active' : ''}" data-event="${id}"><span class="history-index">${index}</span><span><span class="history-title">${title}</span><small>${left}</small></span><span class="history-secondary">${right}</span><time>${time}</time>${icon('chevron')}</button>`).join('');
}

function showToast(message, tone = 'neutral') {
  const region = document.querySelector('.toast-region');
  const toast = document.createElement('div');
  toast.className = `toast toast--${tone}`;
  toast.innerHTML = `${tone === 'success' ? icon('check') : tone === 'warning' ? icon('warning') : ''}<span>${message}</span>`;
  region.append(toast);
  window.setTimeout(() => toast.classList.add('is-leaving'), 2200);
  window.setTimeout(() => toast.remove(), 2400);
}

let alertAudioContext;

function playAlertSound() {
  if (!state.soundEnabled) return;
  try {
    const AudioContext = window.AudioContext || window.webkitAudioContext;
    if (!AudioContext) return;
    alertAudioContext ||= new AudioContext();
    const oscillator = alertAudioContext.createOscillator();
    const gain = alertAudioContext.createGain();
    const now = alertAudioContext.currentTime;
    oscillator.type = 'sine';
    oscillator.frequency.setValueAtTime(660, now);
    oscillator.frequency.exponentialRampToValueAtTime(520, now + 0.1);
    gain.gain.setValueAtTime(0.0001, now);
    gain.gain.exponentialRampToValueAtTime(0.07, now + 0.012);
    gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.12);
    oscillator.connect(gain).connect(alertAudioContext.destination);
    oscillator.start(now);
    oscillator.stop(now + 0.13);
  } catch {
    // Why: sound is optional feedback. A browser audio policy must never interrupt inspection.
  }
}

function bindInteractions() {
  document.querySelector('[data-dismiss-notice]')?.addEventListener('click', (event) => event.currentTarget.closest('.local-notice').remove());
  bindLocalTabs('service-tab', 'serviceTab');
  bindLocalTabs('operation-tab', 'operationTab');
  bindLocalTabs('exchange-tab', 'exchangeTab');
  bindLocalTabs('config-file', 'selectedConfigFile');
  document.querySelectorAll('[data-exchange]').forEach((row) => {
    const open = () => { navigate(`/exchanges/${row.dataset.exchange}`); };
    row.addEventListener('click', open);
    row.addEventListener('keydown', (event) => { if (event.key === 'Enter' || event.key === ' ') open(); });
  });
  document.querySelectorAll('[data-operation]').forEach((row) => {
    const open = () => { navigate(`/operations/${row.dataset.operation}`); };
    row.addEventListener('click', open);
    row.addEventListener('keydown', (event) => { if (event.key === 'Enter' || event.key === ' ') open(); });
  });
  document.querySelectorAll('[data-outcome]').forEach((row) => {
    const toggle = () => {
      state.selectedOutcome = state.selectedOutcome === row.dataset.outcome ? null : row.dataset.outcome;
      renderShell();
      document.querySelector(`[data-outcome="${row.dataset.outcome}"]`)?.focus({preventScroll: true});
    };
    row.addEventListener('click', toggle);
    row.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      event.preventDefault();
      toggle();
    });
  });
  document.querySelectorAll('[data-event]').forEach((button) => button.addEventListener('click', () => { state.selectedEvent = button.dataset.event; renderShell(); document.querySelector('.timeline-view')?.scrollIntoView({block:'start'}); }));
  document.querySelectorAll('[data-stack-component]').forEach((button) => button.addEventListener('click', () => { state.selectedStackComponent = button.dataset.stackComponent; renderShell(); document.querySelector(`[data-stack-component="${state.selectedStackComponent}"]`)?.focus({preventScroll: true}); }));
  document.querySelectorAll('[data-resource]').forEach((button) => button.addEventListener('click', () => { state.selectedResource = button.dataset.resource; renderShell(); }));
  document.querySelectorAll('[data-resource-group]').forEach((button) => button.addEventListener('click', () => {
    state.selectedResourceGroup = button.dataset.resourceGroup;
    const groups = runtime.connected && runtime.inspectionState ? runtimeResourceGroups() : resourceGroups;
    state.selectedResource = groups[state.selectedResourceGroup][0].id;
    renderShell();
  }));
  document.querySelectorAll('[data-scenario]').forEach((button) => button.addEventListener('click', () => { state.selectedScenario = button.dataset.scenario; renderShell(); }));
  document.querySelectorAll('[data-fault-profile]').forEach((button) => button.addEventListener('click', () => { state.selectedFaultProfile = button.dataset.faultProfile; renderShell(); }));
  document.querySelectorAll('[data-copy]').forEach((button) => button.addEventListener('click', async () => { const label = button.dataset.copy; const panel = button.closest('.code-panel, .command-panel, .resource-detail'); const content = panel?.querySelector('code')?.innerText || label; try { await navigator.clipboard.writeText(content); showToast(`Copied ${label}`, 'success'); } catch { showToast('Clipboard access is unavailable', 'warning'); } }));
  document.querySelectorAll('[data-menu]').forEach((button) => button.addEventListener('click', () => openMenu(button)));
  document.querySelectorAll('[data-action]').forEach((button) => button.addEventListener('click', () => handleAction(button.dataset.action)));
  document.querySelectorAll('[data-add-pair]').forEach((button) => button.addEventListener('click', () => updateWorkbenchPairs(button.dataset.addPair)));
  document.querySelectorAll('[data-remove-pair]').forEach((button) => button.addEventListener('click', () => updateWorkbenchPairs(button.dataset.removePair, Number(button.dataset.pairIndex))));
  document.querySelectorAll('[data-code-editor]').forEach((editor) => editor.addEventListener('keydown', handleCodeEditorKeydown));
  document.querySelectorAll('[data-restore-session]').forEach((button) => button.addEventListener('click', () => restoreSession(button.dataset.restoreSession)));
  document.querySelectorAll('[data-delete-session]').forEach((button) => button.addEventListener('click', () => deleteSession(button.dataset.deleteSession)));
  document.querySelector('[data-log-search]')?.addEventListener('input', (event) => {
    state.logQuery = event.currentTarget.value;
    filterLogRows();
  });
  if (state.logQuery) filterLogRows();
  const logBody = document.querySelector('.log-dock__body');
  if (logBody && state.logDock !== 'collapsed') {
    logBody.scrollTop = state.logFollowTail ? logBody.scrollHeight : state.logScrollTop;
    logBody.addEventListener('scroll', () => {
      state.logScrollTop = logBody.scrollTop;
      state.logFollowTail = logBody.scrollHeight - logBody.scrollTop - logBody.clientHeight < 24;
    }, { passive: true });
  }
  document.querySelectorAll('[data-exchange-filter]').forEach((button) => button.addEventListener('click', () => {
    state.exchangeFilter = button.dataset.exchangeFilter;
    document.querySelectorAll('[data-exchange-filter]').forEach((candidate) => {
      const active = candidate.dataset.exchangeFilter === state.exchangeFilter;
      candidate.classList.toggle('is-active', active);
      candidate.setAttribute('aria-pressed', String(active));
    });
    applyExchangeFilters();
  }));
  document.querySelector('[data-exchange-search]')?.addEventListener('input', (event) => {
    state.exchangeQuery = event.currentTarget.value;
    applyExchangeFilters();
  });
  document.querySelectorAll('.switch input').forEach((input) => input.addEventListener('change', () => { const panel = input.closest('.panel'); panel?.querySelector('.disabled-config')?.classList.toggle('is-enabled', input.checked); panel?.querySelectorAll('.disabled-config input, .disabled-config select').forEach((field) => { field.disabled = !input.checked; }); }));
  document.querySelectorAll('.search-field input:not([data-exchange-search])').forEach((input) => input.addEventListener('input', () => filterCurrentList(input.value)));
  if (document.querySelector('[data-exchange-search]')) applyExchangeFilters();
}

function filterLogRows() {
  const normalized = state.logQuery.trim().toLowerCase();
  let visible = 0;
  document.querySelectorAll('.log-entry').forEach((row) => {
    row.hidden = normalized && !row.textContent.toLowerCase().includes(normalized);
    if (!row.hidden) visible += 1;
  });
  const count = document.querySelector('[data-log-count]');
  if (count) count.textContent = String(visible);
}

function bindLocalTabs(dataAttribute, stateKey) {
  const controls = [...document.querySelectorAll(`[data-${dataAttribute}]`)];
  const tabs = controls.filter((control) => control.getAttribute('role') === 'tab');
  const select = (tab) => {
    state[stateKey] = tab.getAttribute(`data-${dataAttribute}`);
    renderShell();
    document.querySelector(`[data-${dataAttribute}="${state[stateKey]}"]`)?.focus({preventScroll: true});
  };
  controls.forEach((control) => control.addEventListener('click', () => select(control)));
  tabs.forEach((tab, index) => {
    tab.addEventListener('keydown', (event) => {
      if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      const nextIndex = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
      select(tabs[nextIndex]);
    });
  });
}

function applyExchangeFilters() {
  const rows = displayedExchangeRows();
  const normalized = state.exchangeQuery.trim().toLowerCase();
  let visible = 0;
  document.querySelectorAll('.exchange-table--modern tbody tr').forEach((row) => {
    const matchesQuery = !normalized || row.textContent.toLowerCase().includes(normalized);
    const matchesStatus = state.exchangeFilter === 'all' || row.dataset.exchangeStatus === state.exchangeFilter;
    row.hidden = !(matchesQuery && matchesStatus);
    if (!row.hidden) visible += 1;
  });
  const resultCount = document.querySelector('[data-exchange-result-count]');
  if (resultCount) resultCount.textContent = `Showing ${visible} of ${rows.length} ${runtime.connected ? 'captured' : 'representative'} exchange${rows.length === 1 ? '' : 's'}`;
}

function filterCurrentList(query) {
  const normalized = query.trim().toLowerCase();
  document.querySelectorAll('.data-table tbody tr, .resource-row, .scenario-row').forEach((row) => { row.hidden = normalized && !row.textContent.toLowerCase().includes(normalized); });
}

function openMenu(button) {
  const popover = document.querySelector('[data-popover]');
  const menus = {
    'service-switcher': `${registeredServices.map((service) => `<a class="${service.name === serviceData.service.name ? 'is-selected' : ''}" href="/?service=${encodeURIComponent(service.name)}"><strong>${service.name}</strong><small>${service.listen} · ${service.version}</small></a>`).join('')}<hr><button data-action="register-service">Register service…</button>`,
    environment: `<button class="${state.environment === 'local' ? 'is-selected' : ''}" data-environment="local"><strong>local</strong><small>${activeProfileName}</small></button><button class="${state.environment === 'test' ? 'is-selected' : ''}" data-environment="test"><strong>fault test</strong><small>${serviceData.faultProfiles[0]?.name}</small></button>`,
    profile: `<button disabled><strong>Yaw</strong><small>Local session</small></button><hr><a href="/settings">Settings</a>`,
    'exchange-actions': `<button data-action="generate-exchange-report">Generate report</button><button data-copy="exchange ID">Copy exchange ID</button><button data-action="download-trace">Download trace</button><button data-action="create-reproduction">Create reproduction</button>`,
    'copy-request': Object.entries(requestFormats).map(([format, { label }]) => `<button class="request-format-option ${state.requestCopyFormat === format ? 'is-selected' : ''}" data-request-format="${format}">${label}</button>`).join(''),
  };
  const rect = button.getBoundingClientRect();
  popover.innerHTML = menus[button.dataset.menu];
  popover.setAttribute('role', 'menu');
  popover.hidden = false;
  button.setAttribute('aria-expanded', 'true');
  popover.style.top = `${rect.bottom + 6}px`;
  popover.style.left = `${Math.min(rect.left, window.innerWidth - 260)}px`;
  popover.style.transformOrigin = `${Math.min(rect.width / 2, 220)}px 0`;
  popover.querySelectorAll('button:not([disabled]), a').forEach((item) => item.setAttribute('role', 'menuitem'));
  popover.querySelectorAll('[data-action]').forEach((item) => item.addEventListener('click', () => handleAction(item.dataset.action)));
  popover.querySelectorAll('[data-copy]').forEach((item) => item.addEventListener('click', async () => {
    const content = item.dataset.copy === 'exchange ID' ? route().detail || exchangeId : item.dataset.copy;
    try { await navigator.clipboard.writeText(content); showToast(`Copied ${item.dataset.copy}`, 'success'); } catch { showToast('Clipboard access is unavailable', 'warning'); }
  }));
  popover.querySelectorAll('[data-request-format]').forEach((item) => item.addEventListener('click', () => {
    state.requestCopyFormat = item.dataset.requestFormat;
    renderShell();
    document.querySelector('[data-action="copy-request"]')?.focus({ preventScroll: true });
  }));
  popover.querySelectorAll('[data-environment]').forEach((item) => item.addEventListener('click', () => { state.environment = item.dataset.environment; renderShell(); showToast(`Switched to ${state.environment}`); }));
  const menuItems = [...popover.querySelectorAll('[role="menuitem"]')];
  menuItems.forEach((item, index) => item.addEventListener('keydown', (event) => {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End', 'Escape'].includes(event.key)) return;
    event.preventDefault();
    if (event.key === 'Escape') {
      popover.hidden = true;
      button.setAttribute('aria-expanded', 'false');
      button.focus({ preventScroll: true });
      return;
    }
    const nextIndex = event.key === 'Home' ? 0 : event.key === 'End' ? menuItems.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + menuItems.length) % menuItems.length;
    menuItems[nextIndex].focus();
  }));
  menuItems[0]?.focus({ preventScroll: true });
  window.setTimeout(() => document.addEventListener('click', closeMenu, {once:true}), 0);
}

function closeMenu(event) {
  if (event.target.closest('[data-popover], [data-menu]')) return;
  document.querySelector('[data-popover]').hidden = true;
  document.querySelectorAll('[data-menu][aria-expanded="true"]').forEach((button) => button.setAttribute('aria-expanded', 'false'));
}

function handleAction(action) {
  const actions = {
    'collapse-sidebar': () => { state.sidebarCollapsed = !state.sidebarCollapsed; renderShell(); },
    'open-console': () => { navigate('/exchanges'); },
    restart: () => showToast('Restart the service from its owning terminal', 'warning'),
    runtime: () => showToast(runtime.connected ? `${serviceData.service.name} inspection is connected` : `Waiting for ${localServiceUrl}`, runtime.connected ? 'success' : 'warning'),
    terminal: () => showToast(`simile start ${serviceData.service.name} · already running`),
    'toggle-theme': () => {
      state.theme = state.theme === 'dark' ? 'light' : 'dark';
      document.documentElement.dataset.theme = state.theme;
      window.localStorage.setItem('simile-theme', state.theme);
      renderShell();
      showToast(`${state.theme === 'light' ? 'Light' : 'Dark'} mode enabled`);
    },
    replay: replayCurrentExchange,
    'run-operation': runOperationWorkbench,
    'create-session': createSession,
    'advance-clock': advanceSessionClock,
    'auto-refresh': () => { state.autoRefresh = !state.autoRefresh; renderShell(); },
    'toggle-log-dock': () => {
      state.logDock = state.logDock === 'collapsed' ? 'partial' : 'collapsed';
      state.logFollowTail = true;
      window.localStorage.setItem('simile-log-dock', state.logDock);
      renderShell();
    },
    'toggle-log-fullscreen': () => {
      state.logDock = state.logDock === 'full' ? 'partial' : 'full';
      state.logFollowTail = true;
      window.localStorage.setItem('simile-log-dock', state.logDock);
      renderShell();
    },
    'toggle-sound': () => {
      state.soundEnabled = !state.soundEnabled;
      window.localStorage.setItem('simile-sound-alerts', state.soundEnabled ? 'enabled' : 'muted');
      if (state.soundEnabled) playAlertSound();
      renderShell();
      showToast(state.soundEnabled ? 'Issue sounds enabled' : 'Issue sounds muted');
    },
    'test-sound': () => { playAlertSound(); showToast('Issue sound played', 'success'); },
    'event-filter': () => { state.eventFilter = state.eventFilter === 'all' ? 'contract' : 'all'; renderShell(); },
    'close-event': () => { document.querySelector('.selected-event')?.remove(); },
    'clear-exchanges': clearExchangeHistory,
    'reset-state': resetRuntimeState,
    'refresh-runtime': async () => {
      await refreshRuntime(true);
      showToast(runtime.connected ? 'Runtime inspection refreshed' : 'Runtime inspection is unavailable', runtime.connected ? 'success' : 'warning');
    },
    'toggle-scenario': () => {
      if (state.disabledScenarios.has(state.selectedScenario)) state.disabledScenarios.delete(state.selectedScenario);
      else state.disabledScenarios.add(state.selectedScenario);
      renderShell();
    },
    'timeline-order': () => { state.timelineOrder = state.timelineOrder === 'ascending' ? 'descending' : 'ascending'; renderShell(); },
    'download-contract': downloadContract,
    'download-exchanges': downloadExchanges,
    'download-trace': downloadTrace,
    'generate-exchange-report': generateExchangeReport,
    'submit-exchange-report': submitExchangeReport,
    'download-exchange-report': downloadExchangeReport,
    'download-report': downloadFidelityReport,
    'export-state': exportRuntimeState,
    'state-command': () => showToast(`simile state ${serviceName} --output json --values`),
    'copy-resource': () => document.querySelector('[data-copy="stored resource"]')?.click(),
    'copy-request': () => copyPreparedRequest(state.requestCopyFormat),
    'resource-actions': () => showToast('Resource actions are available from its operation history'),
    'filter-operations': () => document.querySelector('.search-field input')?.focus(),
    'filter-exchanges': () => document.querySelector('.search-field input')?.focus(),
    'filter-deferred': () => document.querySelector('.search-field input')?.focus(),
    'choose-profile': () => showToast(`${activeProfileName} is the active consumer profile`),
    'create-scenario': () => showToast('Scenario editor is not part of this inspection build'),
    'new-fault': () => showToast('Choose an existing profile or edit the Simfile'),
    'register-service': () => showToast('Run simile register from the terminal'),
    'create-reproduction': generateExchangeReport,
  };
  actions[action]?.();
}

window.addEventListener('popstate', handleNavigation);

document.addEventListener('click', (event) => {
  const link = event.target.closest('a[href]');
  if (!link || link.target || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  const destination = new URL(link.href, window.location.href);
  if (destination.origin !== window.location.origin || destination.search || destination.hash) return;
  const section = destination.pathname.split('/').filter(Boolean)[0] || 'service';
  const known = navItems.some(([key]) => key === section) || ['logs', 'settings', 'docs'].includes(section);
  if (!known) return;
  event.preventDefault();
  navigate(destination.pathname);
});

window.addEventListener('keydown', (event) => {
  const editing = /INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName);
  if (event.key === '/' && !editing) {
    event.preventDefault();
    document.querySelector('.search-field input')?.focus();
  }
  if (!editing && !event.metaKey && !event.ctrlKey && !event.altKey && event.key === 'g') {
    event.preventDefault();
    handleAction('toggle-log-dock');
  }
  if (!editing && !event.metaKey && !event.ctrlKey && !event.altKey && event.key === 'G') {
    event.preventDefault();
    handleAction('toggle-log-fullscreen');
  }
  if (event.key === 'Escape' && state.logDock === 'full') {
    state.logDock = 'partial';
    window.localStorage.setItem('simile-log-dock', state.logDock);
    renderShell();
  } else if (event.key === 'Escape') {
    document.querySelector('[data-popover]')?.setAttribute('hidden', '');
  }
});

renderShell();
refreshRuntime();
window.setInterval(() => {
  if (state.autoRefresh) refreshRuntime();
}, 2000);

async function refreshRuntime(forceRender = false) {
  try {
    const statusResponse = await fetch(`${localServiceUrl}/.simile/inspection`, { cache: 'no-store' });
    if (!statusResponse.ok) throw new Error(`inspection returned ${statusResponse.status}`);
    const status = await statusResponse.json();
    const [exchangeResponse, stateResponse, deferredResponse, control, sessions, controlEvents] = await Promise.all([
      fetch(`${localServiceUrl}/.simile/exchanges`, { cache: 'no-store' }),
      fetch(`${localServiceUrl}/.simile/state`, { cache: 'no-store' }),
      fetch(`${localServiceUrl}/.simile/deferred`, { cache: 'no-store' }),
      inspectorJson('/.simile/console/v1'),
      inspectorJson('/.simile/console/v1/sessions'),
      inspectorJson(`/.simile/console/v1/events?after=${runtime.controlCursor}`),
    ]);
    if (!exchangeResponse.ok || !stateResponse.ok || !deferredResponse.ok) throw new Error('runtime inspection is incomplete');
    const [{ exchanges }, inspectionState, deferred] = await Promise.all([exchangeResponse.json(), stateResponse.json(), deferredResponse.json()]);
    const knownExchangeIds = new Set(runtime.exchanges.map((exchange) => exchange.id));
    const newIssue = runtime.observationsReady && runtime.connected
      ? exchanges.find((exchange) => Number(exchange.status) >= 400 && !knownExchangeIds.has(exchange.id))
      : null;
    const signature = `${status.exchanges}:${status.events}:${exchanges[0]?.id || ''}:${deferred.pending}:${deferred.leased}:${deferred.deadLetters}:${sessions.sessions.length}:${controlEvents.cursor}`;
    const changed = forceRender || !runtime.connected || signature !== runtime.signature;
    runtime.connected = true;
    runtime.status = status;
    runtime.exchanges = exchanges;
    runtime.inspectionState = inspectionState;
    runtime.deferred = deferred;
    runtime.control = control;
    runtime.sessions = sessions.sessions;
    if (controlEvents.events.length) runtime.controlEvents.push(...controlEvents.events);
    runtime.controlEvents = runtime.controlEvents.slice(-256);
    runtime.controlCursor = controlEvents.cursor;
    runtime.error = null;
    runtime.signature = signature;
    runtime.observationsReady = true;
    if (changed) renderShell();
    if (newIssue) {
      showToast(`${newIssue.method} ${newIssue.path} returned ${newIssue.status}`, 'warning');
      playAlertSound();
    }
  } catch (error) {
    const disconnected = runtime.observationsReady && runtime.connected;
    const changed = runtime.connected || runtime.error !== error.message;
    runtime.connected = false;
    runtime.status = null;
    runtime.exchanges = [];
    runtime.inspectionState = null;
    runtime.deferred = null;
    runtime.control = null;
    runtime.sessions = [];
    runtime.error = error.message;
    runtime.signature = '';
    runtime.observationsReady = true;
    if (changed) renderShell();
    if (disconnected) {
      showToast('Local service inspection disconnected', 'warning');
      playAlertSound();
    }
  }
}
