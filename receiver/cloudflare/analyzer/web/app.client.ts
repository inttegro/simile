type ReportStatus = "accepted" | "duplicate" | "pending_verification" | "rejected" | "resolved";

interface ReportSummary {
  total: number;
  statuses: Partial<Record<ReportStatus, number>>;
  recurringFingerprints: number;
}

interface ReportItem {
  id: string;
  status: ReportStatus;
  serviceVersion: string;
  simileVersion: string;
  artifactDigest: string;
  fingerprint: string;
  fingerprintCount: number;
  operation: string;
  title: string;
  kind: string;
  createdAt: string;
  evidenceDeletedAt?: string | null;
  hasContact?: boolean;
  rejectionReason?: string | null;
  resolutionSummary?: string | null;
  reportDocument?: { diagnostic?: Record<string, unknown> } | null;
  request?: unknown;
  bundle?: unknown;
}

interface ReportEvent {
  kind: string;
  actor: string;
  detail: Record<string, unknown>;
  createdAt: string;
}

interface EvidenceItem {
  name: string;
  url: string;
}

interface ReportDetail {
  report: ReportItem;
  events: ReportEvent[];
  evidence: EvidenceItem[];
}

interface AnalyzerState {
  reports: ReportItem[];
  summary: ReportSummary | null;
  detail: ReportDetail | null;
  status: ReportStatus | "";
  search: string;
  loading: boolean;
  detailTab: "overview" | "evidence" | "history";
  theme: "dark" | "light";
}

const state: AnalyzerState = {
  reports: [],
  summary: null,
  detail: null,
  status: "",
  search: "",
  loading: true,
  detailTab: "overview",
  theme: document.documentElement.dataset.theme === "light" ? "light" : "dark",
};

const icons = {
  activity: '<path d="M3 12h4l2.5-7 5 14 2.5-7h4"/>',
  check: '<path d="m5 12 4 4L19 6"/>',
  chevron: '<path d="m9 18 6-6-6-6"/>',
  copy: '<rect x="8" y="8" width="11" height="11" rx="1"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>',
  download: '<path d="M12 3v12M7 10l5 5 5-5M4 21h16"/>',
  filter: '<path d="M4 5h16l-6 7v5l-4 2v-7L4 5Z"/>',
  logo: '<path d="M8.1 4.4a5.2 5.2 0 0 0 0 10.4h2.1v-2.7H8.1a2.5 2.5 0 1 1 0-5h5.8a2.5 2.5 0 1 1 0 5h-2.1v2.7h2.1a5.2 5.2 0 1 0 0-10.4H8.1Z"/><path d="M15.9 9.2h-2.1v2.7h2.1a2.5 2.5 0 1 1 0 5H10a2.5 2.5 0 1 1 0-5h2.1V9.2H10a5.2 5.2 0 1 0 0 10.4h5.8a5.2 5.2 0 1 0 0-10.4Z"/>',
  refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6"/>',
  reports: '<path d="M5 3h11a3 3 0 0 1 3 3v15H8a3 3 0 0 1-3-3V3Z"/><path d="M8 17h11M9 7h6M9 11h6"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/>',
  shield: '<path d="M12 3 4 6v5c0 5 3.4 8.4 8 10 4.6-1.6 8-5 8-10V6l-8-3Z"/><path d="m8.5 12 2.2 2.2 4.8-5"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  moon: '<path d="M20.7 15.1A8.5 8.5 0 0 1 8.9 3.3 8.5 8.5 0 1 0 20.7 15.1Z"/>',
  user: '<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>',
  warning: '<path d="M12 3 2.5 20h19L12 3Z"/><path d="M12 9v4M12 17h.01"/>',
} as const;

function icon(name: keyof typeof icons, className = ""): string {
  return `<svg class="icon ${className}" viewBox="0 0 24 24" aria-hidden="true">${icons[name]}</svg>`;
}

function escapeHtml(value: unknown): string {
  const entities: Record<string, string> = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  };
  return String(value ?? "").replace(/[&<>"']/g, (character) => entities[character]!);
}

function route(): { reportId: string | null } {
  const value = window.location.hash.replace(/^#\/?/, "");
  const match = value.match(/^reports\/(rpt_[a-z0-9_]+)$/);
  return { reportId: match?.[1] || null };
}

async function api<T>(path: string, options?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...options,
    headers: { ...(options?.body ? { "content-type": "application/json" } : {}), ...options?.headers },
  });
  const contentType = response.headers.get("content-type") || "";
  const body: unknown = contentType.includes("application/json")
    ? await response.json()
    : await response.text();
  if (!response.ok) {
    const message =
      typeof body === "object" && body !== null && "error" in body
        ? (body as { error?: { message?: string } }).error?.message
        : typeof body === "string"
          ? body
          : null;
    throw new Error(message || `Request failed with HTTP ${response.status}`);
  }
  return body as T;
}

async function loadList({ quiet = false }: { quiet?: boolean } = {}): Promise<void> {
  if (!quiet) state.loading = true;
  render();
  const query = new URLSearchParams({ limit: "60" });
  if (state.status) query.set("status", state.status);
  if (state.search) query.set("search", state.search);
  try {
    const [summary, result] = await Promise.all([
      api<ReportSummary>("/api/summary"),
      api<{ reports: ReportItem[] }>(`/api/reports?${query}`),
    ]);
    state.summary = summary;
    state.reports = result.reports;
    state.loading = false;
    render();
  } catch (error) {
    state.loading = false;
    renderError(error);
  }
}

async function loadDetail(reportId: string): Promise<void> {
  state.loading = true;
  state.detail = null;
  render();
  try {
    state.detail = await api<ReportDetail>(`/api/reports/${encodeURIComponent(reportId)}`);
    state.loading = false;
    render();
  } catch (error) {
    state.loading = false;
    renderError(error);
  }
}

function render(): void {
  const current = route();
  const app = elementById<HTMLElement>("app");
  app.className = "app";
  app.innerHTML = shell(
    current.reportId ? reportPage(current.reportId) : reportsPage(),
    Boolean(current.reportId),
  );
  bind();
}

function shell(content: string, detail: boolean): string {
  return `<header class="topbar"><div class="topbar__inner">
      <a class="service-switcher" href="#reports" aria-label="Simile reports">${icon("logo", "logo")}<span>simile</span><span class="service-revision">reports</span></a>
      <div class="topbar__rail"><div class="local-context"><span class="live-dot"></span><strong>Report receiver</strong><span>Cloudflare</span></div></div>
      <div class="topbar__actions">
        <span class="runtime-status"><span class="live-dot"></span>Connected</span>
        <span class="environment">${icon("shield")}private review</span>
        <button class="top-icon theme-toggle" type="button" data-theme-toggle aria-label="Switch to ${state.theme === "dark" ? "light" : "dark"} mode" title="Switch to ${state.theme === "dark" ? "light" : "dark"} mode">${icon(state.theme === "dark" ? "sun" : "moon")}</button>
        <span class="avatar" aria-label="Simile reports">SR</span>
      </div>
    </div></header>
    <div class="product-nav"><div class="product-nav__inner">
      <nav class="primary-nav" aria-label="Report analyzer">
        <a href="#reports" class="nav-link ${detail ? "" : "is-active"}" ${detail ? "" : 'aria-current="page"'}>${icon("reports")}<span>Reports</span></a>
      </nav>
      <nav class="secondary-nav" aria-label="Analyzer context"><span class="nav-link report-context">Inttegro service reports</span></nav>
    </div></div>
    <main class="workspace" id="workspace" tabindex="-1">${content}</main>`;
}

function reportsPage(): string {
  const summary = state.summary || { total: 0, statuses: {}, recurringFingerprints: 0 };
  const filters = [
    // Why: the API represents the unfiltered view by omitting `status`; sending a synthetic
    // `all` status makes the strict filter validator reject an otherwise valid navigation.
    ["", "All", summary.total],
    ["pending_verification", "Pending", summary.statuses.pending_verification || 0],
    ["accepted", "Accepted", summary.statuses.accepted || 0],
    ["rejected", "Rejected", summary.statuses.rejected || 0],
    ["resolved", "Resolved", summary.statuses.resolved || 0],
  ];
  return `<div class="page">
    <header class="page-header"><div><h1>Reports</h1><p>Reproducible emulator inconsistencies submitted for independent verification.</p></div><button class="button" type="button" data-refresh>${icon("refresh")}Refresh</button></header>
    <section class="status-strip report-status-strip" aria-label="Report status summary">
      <div><span class="live-dot"></span><span><strong>${summary.total} reports</strong><small>${summary.recurringFingerprints} recurring fingerprint${summary.recurringFingerprints === 1 ? "" : "s"}</small></span></div>
      <dl>
        <div><dt>Pending</dt><dd>${summary.statuses.pending_verification || 0}</dd></div>
        <div><dt>Accepted</dt><dd>${summary.statuses.accepted || 0}</dd></div>
        <div><dt>Rejected</dt><dd>${summary.statuses.rejected || 0}</dd></div>
        <div><dt>Resolved</dt><dd>${summary.statuses.resolved || 0}</dd></div>
      </dl>
    </section>
    <div class="toolbar report-toolbar">
      <label class="search-field">${icon("search")}<input type="search" value="${escapeHtml(state.search)}" placeholder="Search report, operation, or fingerprint" aria-label="Search reports" data-search></label>
      <div class="toolbar__end"><div class="segmented-filter" aria-label="Filter reports by status">${filters.map(([filter, label, count]) => `<button type="button" class="${state.status === filter ? "is-active" : ""}" data-status="${filter}">${label}<span>${count}</span></button>`).join("")}</div><span class="result-count">${state.reports.length} shown</span></div>
    </div>
    <section class="table-panel report-ledger">${state.loading ? loadingRows() : reportTable()}</section>
  </div>`;
}

function reportTable(): string {
  if (state.reports.length === 0) {
    return `<div class="empty-state">${icon("check")}<h2>No matching reports</h2><p>The receiver has no reports for the current filters.</p></div>`;
  }
  return `<div class="table-scroll"><table class="data-table report-table"><thead><tr><th>Report</th><th>Operation</th><th>Status</th><th>Recurrence</th><th>Received</th><th></th></tr></thead><tbody>${state.reports.map((report) => `<tr class="clickable-row report-row" tabindex="0" data-report="${escapeHtml(report.id)}"><td><strong>${escapeHtml(report.title || report.id)}</strong><code>${escapeHtml(report.id)}</code></td><td><code>${escapeHtml(report.operation || "unknown")}</code><span>${escapeHtml(report.serviceVersion)}</span></td><td>${statusBadge(report.status)}</td><td>${report.fingerprintCount > 1 ? `<strong>${report.fingerprintCount} reports</strong>` : "First occurrence"}</td><td><time datetime="${escapeHtml(report.createdAt)}">${formatTime(report.createdAt)}</time></td><td class="report-row-action">${icon("chevron")}</td></tr>`).join("")}</tbody></table></div><footer class="table-footer"><span>${state.reports.length} reports shown</span><span>Independent reproduction required</span></footer>`;
}

function reportPage(reportId: string): string {
  if (state.loading || !state.detail) return `<div class="page">${backLink()}${detailSkeleton()}</div>`;
  const { report, events, evidence } = state.detail;
  if (report.id !== reportId) return detailSkeleton();
  return `<div class="page report-detail">
    ${backLink()}
    <header class="detail-header"><div><div class="eyebrow">${statusBadge(report.status)}<span>${escapeHtml(report.kind || "report")}</span></div><h1>${escapeHtml(report.title || report.id)}</h1><p><code>${escapeHtml(report.id)}</code> · received ${formatTime(report.createdAt)}</p></div><div class="detail-actions"><button class="button" type="button" data-contact ${report.hasContact ? "" : "disabled"}>${icon("user")}Reveal contact</button><button class="button" type="button" data-action="reverify" ${report.evidenceDeletedAt ? "disabled" : ""}>${icon("refresh")}Re-run verification</button></div></header>
    <dl class="fact-grid">
      <div><dt>Operation</dt><dd><code>${escapeHtml(report.operation || "unknown")}</code></dd></div>
      <div><dt>Service version</dt><dd>${escapeHtml(report.serviceVersion)}</dd></div>
      <div><dt>Simile version</dt><dd>${escapeHtml(report.simileVersion)}</dd></div>
      <div><dt>Occurrences</dt><dd>${report.fingerprintCount}</dd></div>
      <div class="fact-wide"><dt>Artifact</dt><dd><code>${escapeHtml(report.artifactDigest)}</code><button class="copy-button" data-copy="${escapeHtml(report.artifactDigest)}" aria-label="Copy artifact digest">${icon("copy")}</button></dd></div>
      <div class="fact-wide"><dt>Fingerprint</dt><dd><code>${escapeHtml(report.fingerprint)}</code><button class="copy-button" data-copy="${escapeHtml(report.fingerprint)}" aria-label="Copy fingerprint">${icon("copy")}</button></dd></div>
    </dl>
    ${report.rejectionReason ? `<aside class="notice notice--danger">${icon("warning")}<div><strong>Verification did not reproduce</strong><p>${escapeHtml(report.rejectionReason)}</p></div></aside>` : ""}
    ${report.resolutionSummary ? `<aside class="notice notice--success">${icon("check")}<div><strong>Resolution</strong><p>${escapeHtml(report.resolutionSummary)}</p></div></aside>` : ""}
    <nav class="subtabs" aria-label="Report details">${[["overview", "Overview"], ["evidence", `Evidence ${evidence.length}`], ["history", `History ${events.length}`]].map(([key, label]) => `<button type="button" class="${state.detailTab === key ? "is-active" : ""}" data-tab="${key}">${label}</button>`).join("")}</nav>
    <section class="tab-panel">${detailTab(report, events, evidence)}</section>
    <section class="review-panel"><header><div><h2>Review outcome</h2><p>Record a deliberate disposition after inspecting verification and evidence.</p></div></header><div class="review-actions"><button class="button button--primary" data-open-action="resolve" ${["accepted", "rejected", "duplicate"].includes(report.status) ? "" : "disabled"}>Resolve</button><button class="button" data-open-action="duplicate">Mark duplicate</button><button class="button button--danger" data-open-action="reject">Reject</button></div></section>
  </div>`;
}

function detailTab(report: ReportItem, events: ReportEvent[], evidence: EvidenceItem[]): string {
  if (state.detailTab === "evidence") {
    if (report.evidenceDeletedAt) return `<div class="empty-state compact">${icon("warning")}<h2>Evidence expired</h2><p>Retention removed the quarantined technical files on ${formatTime(report.evidenceDeletedAt)}.</p></div>`;
    return `<div class="evidence-grid">${evidence.map((item) => `<a class="evidence-file" href="${escapeHtml(item.url)}"><span class="file-icon">${icon("download")}</span><span><strong>${escapeHtml(item.name)}</strong><small>${evidenceDescription(item.name)}</small></span>${icon("chevron")}</a>`).join("")}</div>`;
  }
  if (state.detailTab === "history") {
    return `<ol class="timeline">${events.map((event) => `<li><span class="timeline-dot"></span><div><header><strong>${eventLabel(event.kind)}</strong><time datetime="${escapeHtml(event.createdAt)}">${formatTime(event.createdAt)}</time></header><p>${escapeHtml(event.actor)}</p>${Object.keys(event.detail || {}).length ? `<pre><code>${escapeHtml(JSON.stringify(event.detail, null, 2))}</code></pre>` : ""}</div></li>`).join("")}</ol>`;
  }
  const diagnostic = report.reportDocument?.diagnostic;
  return `<div class="overview-grid">
    <section class="panel"><header><h2>Diagnostic</h2></header>${diagnostic ? `<dl class="stacked-facts"><div><dt>Code</dt><dd><code>${escapeHtml(diagnostic.code)}</code></dd></div><div><dt>Message</dt><dd>${escapeHtml(diagnostic.message)}</dd></div></dl>${jsonBlock("Issues", diagnostic.issues || [])}` : `<p class="muted">No report diagnostic is available.</p>`}</section>
    <section class="panel"><header><h2>Captured request</h2></header>${report.request ? jsonBlock("request.json", report.request) : `<p class="muted">Request evidence is unavailable.</p>`}</section>
    <section class="panel panel--wide"><header><h2>Reproduction controls</h2></header>${report.bundle ? jsonBlock("bundle.json", report.bundle) : `<p class="muted">Bundle evidence is unavailable.</p>`}</section>
  </div>`;
}

function jsonBlock(label: string, value: unknown): string {
  return `<div class="json-block"><header><span>${escapeHtml(label)}</span><button class="copy-button" data-copy="${escapeHtml(JSON.stringify(value))}" aria-label="Copy ${escapeHtml(label)}">${icon("copy")}</button></header><pre><code>${escapeHtml(JSON.stringify(value, null, 2))}</code></pre></div>`;
}

function bind(): void {
  document.querySelector("[data-theme-toggle]")?.addEventListener("click", () => {
    state.theme = state.theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = state.theme;
    window.localStorage.setItem("simile-theme", state.theme);
    render();
    toast(`${state.theme === "light" ? "Light" : "Dark"} mode enabled`);
  });
  document.querySelectorAll("[data-report]").forEach((row) => {
    const reportRow = row as HTMLElement;
    const open = () => { window.location.hash = `reports/${reportRow.dataset.report}`; };
    row.addEventListener("click", open);
    row.addEventListener("keydown", (event) => {
      const keyboardEvent = event as KeyboardEvent;
      if (keyboardEvent.key === "Enter" || keyboardEvent.key === " ") open();
    });
  });
  document.querySelector("[data-refresh]")?.addEventListener("click", () => loadList());
  document.querySelector("[data-status-select]")?.addEventListener("change", (event) => {
    state.status = (event.target as HTMLSelectElement).value as ReportStatus | "";
    loadList();
  });
  document.querySelectorAll<HTMLElement>("[data-status]").forEach((button) => button.addEventListener("click", () => {
    state.status = (button.dataset.status || "") as ReportStatus | "";
    loadList();
  }));
  let timer: number | undefined;
  document.querySelector("[data-search]")?.addEventListener("input", (event) => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      state.search = (event.target as HTMLInputElement).value.trim();
      loadList();
    }, 250);
  });
  document.querySelectorAll<HTMLElement>("[data-tab]").forEach((button) => button.addEventListener("click", () => {
    state.detailTab = (button.dataset.tab || "overview") as AnalyzerState["detailTab"];
    render();
  }));
  document.querySelectorAll<HTMLElement>("[data-copy]").forEach((button) => button.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(button.dataset.copy || "");
      toast("Copied to clipboard", "success");
    } catch {
      toast("Clipboard access is unavailable", "warning");
    }
  }));
  document.querySelector("[data-action='reverify']")?.addEventListener("click", () => runAction({ action: "reverify" }));
  document.querySelector("[data-contact]")?.addEventListener("click", revealContact);
  document.querySelectorAll<HTMLElement>("[data-open-action]").forEach((button) => button.addEventListener("click", () => {
    const action = button.dataset.openAction;
    if (action === "resolve" || action === "reject" || action === "duplicate") {
      openActionDialog(action);
    }
  }));
}

async function runAction(body: Record<string, unknown>): Promise<void> {
  const reportId = route().reportId;
  if (!reportId) return;
  try {
    await api(`/api/reports/${encodeURIComponent(reportId)}/actions`, {
      method: "POST",
      body: JSON.stringify(body),
    });
    toast("Report updated", "success");
    await loadDetail(reportId);
  } catch (error) {
    toast(errorMessage(error), "danger");
  }
}

async function revealContact(): Promise<void> {
  const reportId = route().reportId;
  if (!reportId) return;
  try {
    const contact = await api<{ channel: string; address: string }>(
      `/api/reports/${encodeURIComponent(reportId)}/contact`,
      { method: "POST" },
    );
    openContactDialog(contact);
  } catch (error) {
    toast(errorMessage(error), "danger");
  }
}

function openActionDialog(action: "resolve" | "reject" | "duplicate"): void {
  const dialog = elementById<HTMLDialogElement>("action-dialog");
  const configs = {
    resolve: ["Resolve report", "Describe the fix or conclusion", "summary", "Resolution summary"],
    reject: ["Reject report", "Explain why this evidence should not enter the issue workflow", "reason", "Rejection reason"],
    duplicate: ["Mark as duplicate", "Enter the canonical report identifier", "duplicateOf", "rpt_…"],
  } as const satisfies Record<typeof action, readonly [string, string, string, string]>;
  const config = configs[action];
  dialog.innerHTML = `<form method="dialog"><header><div><h2>${config[0]}</h2><p>${config[1]}</p></div><button class="dialog-close" value="cancel" aria-label="Close">×</button></header><label><span>${config[3]}</span>${action === "duplicate" ? `<input name="value" required pattern="rpt_[a-z0-9_]+" placeholder="rpt_…">` : `<textarea name="value" required rows="5"></textarea>`}</label><footer><button class="button" value="cancel">Cancel</button><button class="button ${action === "reject" ? "button--danger" : "button--primary"}" value="confirm">Confirm</button></footer></form>`;
  dialog.addEventListener("close", () => {
    if (dialog.returnValue !== "confirm") return;
    const form = dialog.querySelector<HTMLFormElement>("form");
    if (!form) return;
    const value = new FormData(form).get("value");
    runAction({ action: action === "duplicate" ? "mark_duplicate" : action, [config[2]]: value });
  }, { once: true });
  dialog.showModal();
  dialog.querySelector<HTMLElement>("textarea, input")?.focus();
}

function openContactDialog(contact: { channel: string; address: string }): void {
  const dialog = elementById<HTMLDialogElement>("action-dialog");
  dialog.innerHTML = `<form method="dialog"><header><div><h2>Reporter contact</h2><p>This access has been recorded in the report history.</p></div><button class="dialog-close" value="close" aria-label="Close">×</button></header><div class="contact-value"><span>${escapeHtml(contact.channel)}</span><strong>${escapeHtml(contact.address)}</strong><button type="button" class="copy-button" data-dialog-copy="${escapeHtml(contact.address)}">${icon("copy")}</button></div><footer><button class="button button--primary" value="close">Done</button></footer></form>`;
  dialog.querySelector<HTMLElement>("[data-dialog-copy]")?.addEventListener("click", async (event) => {
    await navigator.clipboard.writeText((event.currentTarget as HTMLElement).dataset.dialogCopy || "");
    toast("Contact copied", "success");
  });
  dialog.showModal();
}

function statusBadge(status: string): string {
  const tone = {
    accepted: "success",
    duplicate: "neutral",
    pending_verification: "warning",
    rejected: "danger",
    resolved: "info",
  }[status as ReportStatus] || "neutral";
  return `<span class="badge badge--${tone}"><i></i>${escapeHtml(statusLabel(status))}</span>`;
}

function statusLabel(status: string): string {
  return String(status || "unknown").replaceAll("_", " ").replace(/^./, (character) => character.toUpperCase());
}

function eventLabel(kind: string): string {
  return String(kind || "event").replaceAll("_", " ").replace(/^./, (character) => character.toUpperCase());
}

function evidenceDescription(name: string): string {
  const descriptions = {
    "bundle.json": "Reproduction controls and expected diagnostic",
    "report.json": "Submitted technical report",
    "request.body": "Sanitized request body",
    "request.json": "Normalized request metadata",
    "service.similec": "Exact compiled service artifact",
  } as const;
  return descriptions[name as keyof typeof descriptions] || "Report evidence";
}

function formatTime(value: string | null | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return escapeHtml(value);
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

function backLink(): string {
  return `<a class="back-link" href="#reports">‹ Back to reports</a>`;
}

function loadingRows(): string {
  return `<div class="loading-list" aria-label="Loading reports">${Array.from({ length: 6 }, () => '<div><span></span><span></span><span></span><span></span></div>').join("")}</div>`;
}

function detailSkeleton(): string {
  return `<div class="detail-skeleton"><span></span><span></span><span></span><div></div></div>`;
}

function renderError(error: unknown): void {
  const app = elementById<HTMLElement>("app");
  app.className = "app";
  app.innerHTML = shell(`<div class="page"><div class="error-state">${icon("warning")}<h1>Could not load reports</h1><p>${escapeHtml(errorMessage(error))}</p><button class="button button--primary" data-retry>Try again</button></div></div>`, false);
  document.querySelector("[data-retry]")?.addEventListener("click", () => loadList());
}

function toast(message: string, tone = "neutral"): void {
  const region = elementById<HTMLElement>("toasts");
  const item = document.createElement("div");
  item.className = `toast toast--${tone}`;
  item.textContent = message;
  region.append(item);
  window.setTimeout(() => item.remove(), 3600);
}

function elementById<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`required interface element #${id} is missing`);
  return element as T;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "An unexpected error occurred";
}

window.addEventListener("hashchange", () => {
  state.detailTab = "overview";
  const current = route();
  if (current.reportId) loadDetail(current.reportId);
  else loadList();
});

window.setInterval(() => {
  if (!route().reportId && document.visibilityState === "visible") loadList({ quiet: true });
}, 15_000);

if (!window.location.hash) {
  window.location.hash = "reports";
} else {
  const initial = route();
  if (initial.reportId) loadDetail(initial.reportId);
  else loadList();
}
