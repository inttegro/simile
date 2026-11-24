use std::collections::HashSet;
use std::io::{self, IsTerminal, Write as _};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Frame;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap};
use serde::Deserialize;
use serde_json::{Value, json};
use simile_core::ops;
use simile_core::{ReportDocument, ReportKind, ReportTrigger};
use time::{OffsetDateTime, UtcOffset};

use super::ConsoleSession;
use crate::error::{Error, Result};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const NOTICE_TTL: Duration = Duration::from_secs(4);
const SIDEBAR_WIDTH: u16 = 22;

const BACKGROUND: Color = Color::Rgb(0, 0, 0);
const SURFACE: Color = Color::Rgb(9, 9, 9);
const BORDER: Color = Color::Rgb(38, 38, 38);
const BORDER_STRONG: Color = Color::Rgb(58, 58, 58);
const TEXT: Color = Color::Rgb(245, 245, 245);
const MUTED: Color = Color::Rgb(139, 146, 151);
const GREEN: Color = Color::Rgb(40, 232, 117);
const GREEN_MUTED: Color = Color::Rgb(18, 67, 37);
const BLUE: Color = Color::Rgb(92, 168, 255);
const CYAN: Color = Color::Rgb(51, 214, 255);
const AMBER: Color = Color::Rgb(255, 189, 74);
const RED: Color = Color::Rgb(255, 104, 120);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Page {
    Service,
    Operations,
    Exchanges,
    State,
    Deferred,
    Scenarios,
    Faults,
    Reports,
    Fidelity,
    Settings,
}

impl Page {
    const ALL: [Self; 10] = [
        Self::Service,
        Self::Operations,
        Self::Exchanges,
        Self::State,
        Self::Deferred,
        Self::Scenarios,
        Self::Faults,
        Self::Reports,
        Self::Fidelity,
        Self::Settings,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Service => "Service",
            Self::Operations => "Operations",
            Self::Exchanges => "Exchanges",
            Self::State => "State",
            Self::Deferred => "Deferred work",
            Self::Scenarios => "Scenarios",
            Self::Faults => "Faults",
            Self::Reports => "Reports",
            Self::Fidelity => "Fidelity",
            Self::Settings => "Settings",
        }
    }

    const fn shortcut(self) -> &'static str {
        match self {
            Self::Service => "1",
            Self::Operations => "2",
            Self::Exchanges => "3",
            Self::State => "4",
            Self::Deferred => "5",
            Self::Scenarios => "6",
            Self::Faults => "7",
            Self::Reports => "R",
            Self::Fidelity => "8",
            Self::Settings => "9",
        }
    }

    const fn symbol(self) -> &'static str {
        match self {
            Self::Service => "◫",
            Self::Operations => "⌘",
            Self::Exchanges => "⇄",
            Self::State => "◇",
            Self::Deferred => "◷",
            Self::Scenarios => "◎",
            Self::Faults => "⚡",
            Self::Reports => "▤",
            Self::Fidelity => "✓",
            Self::Settings => "⚙",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Service => "Runtime, contract, and composition at a glance",
            Self::Operations => "Executable operations and their compiled outcomes",
            Self::Exchanges => "Sanitized requests captured by this local runtime",
            Self::State => "Read-only resources owned by the running service",
            Self::Deferred => "Declared and queued asynchronous consequences",
            Self::Scenarios => "Named, deterministic test outcomes",
            Self::Faults => "Bounded transport failures available to tests",
            Self::Reports => "Prepared inconsistency reports and verification evidence",
            Self::Fidelity => "Exact behavior, synthesis, and visible contract gaps",
            Self::Settings => "Service configuration and local console preferences",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }

    const fn numbered(number: char) -> Option<Self> {
        match number {
            '1' => Some(Self::Service),
            '2' => Some(Self::Operations),
            '3' => Some(Self::Exchanges),
            '4' => Some(Self::State),
            '5' => Some(Self::Deferred),
            '6' => Some(Self::Scenarios),
            '7' => Some(Self::Faults),
            '8' => Some(Self::Fidelity),
            '9' => Some(Self::Settings),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Focus {
    Navigation,
    List,
    Detail,
    Logs,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum DetailMode {
    #[default]
    Split,
    Expanded,
    Full,
}

impl DetailMode {
    const fn toggle(self) -> Self {
        match self {
            Self::Split => Self::Expanded,
            Self::Expanded | Self::Full => Self::Split,
        }
    }

    const fn toggle_full(self) -> Self {
        match self {
            Self::Full => Self::Expanded,
            Self::Split | Self::Expanded => Self::Full,
        }
    }

    const fn restore_index(self) -> Self {
        match self {
            Self::Full => Self::Expanded,
            Self::Split | Self::Expanded => self,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsDocument {
    Simfile,
    Contract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceSyntax {
    Hcl,
    Contract,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum LogDock {
    #[default]
    Collapsed,
    Partial,
    Full,
}

impl LogDock {
    const fn toggle(self) -> Self {
        match self {
            Self::Collapsed => Self::Partial,
            Self::Partial | Self::Full => Self::Collapsed,
        }
    }

    const fn toggle_full(self) -> Self {
        match self {
            Self::Full => Self::Partial,
            Self::Collapsed | Self::Partial => Self::Full,
        }
    }

    const fn is_expanded(self) -> bool {
        !matches!(self, Self::Collapsed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Confirmation {
    ClearExchanges,
    ResetState,
}

impl Confirmation {
    const fn title(self) -> &'static str {
        match self {
            Self::ClearExchanges => "Clear exchange history?",
            Self::ResetState => "Reset local service state?",
        }
    }

    const fn detail(self) -> &'static str {
        match self {
            Self::ClearExchanges => {
                "Every captured request and response for this service will be removed."
            }
            Self::ResetState => {
                "Every local resource, journal entry, replay, and queued item will be removed."
            }
        }
    }
}

struct App {
    page: Page,
    cursors: [usize; Page::ALL.len()],
    focus: Focus,
    service_cursor: usize,
    selecting_service: bool,
    detail_scroll: u16,
    detail_mode: DetailMode,
    operation_outcome_cursor: usize,
    expanded_outcome: Option<usize>,
    settings_document: Option<SettingsDocument>,
    query: String,
    query_mode: bool,
    auto_refresh: bool,
    log_dock: LogDock,
    log_scroll: usize,
    sound_enabled: bool,
    observations_ready: bool,
    known_exchange_ids: HashSet<String>,
    show_help: bool,
    confirmation: Option<Confirmation>,
    runtime: RuntimeViews,
    refreshed_at: Instant,
    notice: Option<(String, Instant, Color)>,
}

impl App {
    fn cursor(&self) -> usize {
        self.cursors[self.page.index()]
    }

    fn set_cursor(&mut self, cursor: usize) {
        self.cursors[self.page.index()] = cursor;
        self.detail_scroll = 0;
        self.operation_outcome_cursor = 0;
        self.expanded_outcome = None;
    }

    fn move_cursor(&mut self, delta: isize, count: usize) {
        if count == 0 {
            self.set_cursor(0);
            return;
        }
        let current = self.cursor().min(count - 1);
        self.set_cursor(current.saturating_add_signed(delta).min(count - 1));
    }

    fn select_page(&mut self, page: Page) {
        self.page = page;
        self.focus = Focus::Navigation;
        self.detail_scroll = 0;
        self.detail_mode = DetailMode::Split;
        self.settings_document = None;
        self.query.clear();
        self.query_mode = false;
    }

    fn cycle_page(&mut self, delta: isize) {
        let current = self.page.index();
        let next = if delta.is_negative() {
            current
                .checked_sub(delta.unsigned_abs())
                .unwrap_or(Page::ALL.len() - 1)
        } else {
            (current + delta.unsigned_abs()) % Page::ALL.len()
        };
        self.select_page(Page::ALL[next]);
    }

    fn set_notice(&mut self, message: impl Into<String>, color: Color) {
        self.notice = Some((message.into(), Instant::now(), color));
    }

    fn notice(&self) -> Option<(&str, Color)> {
        self.notice.as_ref().and_then(|(message, created, color)| {
            (created.elapsed() < NOTICE_TTL).then_some((message.as_str(), *color))
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionSummary {
    status: String,
    resources: usize,
    events: usize,
    exchanges: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Exchange {
    id: String,
    operation: String,
    method: String,
    path: String,
    started_at_ms: u64,
    duration_micros: u64,
    status: u16,
    outcome: Option<String>,
    profile: Option<String>,
    request: Message,
    response: Message,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    content_type: Option<String>,
    body: Option<Value>,
    truncated: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct ExchangeHistory {
    #[serde(default)]
    exchanges: Vec<Exchange>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct StateView {
    #[serde(default)]
    resources: Vec<ResourceView>,
    #[serde(default)]
    events: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize)]
struct ResourceView {
    address: ResourceAddress,
    value: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
struct ResourceAddress {
    kind: String,
    name: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeferredView {
    pending: usize,
    leased: usize,
    dead_letters: usize,
    #[serde(default)]
    items: Vec<DeferredItem>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeferredItem {
    id: String,
    queue: String,
    receive_count: u32,
    leased: bool,
    work: Value,
}

#[derive(Clone, Debug)]
struct LogEntry {
    timestamp_ms: i64,
    level: &'static str,
    source: &'static str,
    summary: String,
    context: String,
}

#[derive(Default)]
struct RuntimeViews {
    inspection: Option<InspectionSummary>,
    exchanges: ExchangeHistory,
    state: StateView,
    deferred: DeferredView,
    reports: Vec<ReportDocument>,
}

impl RuntimeViews {
    fn connected(&self) -> bool {
        self.inspection.is_some()
    }
}

pub(crate) fn run(runtime: &tokio::runtime::Runtime, session: &mut ConsoleSession) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::ConsoleTerminalRequired);
    }
    enable_raw_mode().map_err(Error::ConsoleRender)?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(Error::ConsoleRender)?;
    let _restore = RestoreTerminal;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend).map_err(Error::ConsoleRender)?;
    terminal.clear().map_err(Error::ConsoleRender)?;

    let service_cursor = session
        .catalog()
        .services
        .iter()
        .position(|service| service.name == session.selected().service.name)
        .unwrap_or_default();
    let mut app = App {
        page: Page::Service,
        cursors: [0; Page::ALL.len()],
        focus: Focus::Navigation,
        service_cursor,
        selecting_service: false,
        detail_scroll: 0,
        detail_mode: DetailMode::Split,
        operation_outcome_cursor: 0,
        expanded_outcome: None,
        settings_document: None,
        query: String::new(),
        query_mode: false,
        auto_refresh: true,
        log_dock: LogDock::Collapsed,
        log_scroll: 0,
        sound_enabled: true,
        observations_ready: false,
        known_exchange_ids: HashSet::new(),
        show_help: false,
        confirmation: None,
        runtime: RuntimeViews::default(),
        refreshed_at: Instant::now() - REFRESH_INTERVAL,
        notice: None,
    };
    let mut dirty = true;

    loop {
        if app.auto_refresh && app.refreshed_at.elapsed() >= REFRESH_INTERVAL {
            let next = runtime.block_on(refresh_runtime(session));
            if apply_runtime_refresh(&mut app, next, true) {
                ring_terminal();
            }
            app.refreshed_at = Instant::now();
            dirty = true;
        }
        if app
            .notice
            .as_ref()
            .is_some_and(|(_, created, _)| created.elapsed() >= NOTICE_TTL)
        {
            app.notice = None;
            dirty = true;
        }
        if dirty {
            terminal
                .draw(|frame| draw(frame, session, &app))
                .map_err(Error::ConsoleRender)?;
            dirty = false;
        }

        if !event::poll(Duration::from_millis(100)).map_err(Error::ConsoleEvent)? {
            continue;
        }
        dirty = true;
        match event::read().map_err(Error::ConsoleEvent)? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if handle_key(key, runtime, session, &mut app) {
                    break;
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => move_vertical(&mut app, session.selected(), 1),
                MouseEventKind::ScrollUp => move_vertical(&mut app, session.selected(), -1),
                MouseEventKind::Down(_)
                    if log_dock_header_row(&app).is_some_and(|row| row == mouse.row) =>
                {
                    app.log_dock = app.log_dock.toggle();
                    app.focus = if app.log_dock.is_expanded() {
                        Focus::Logs
                    } else {
                        Focus::Navigation
                    };
                    app.log_scroll = 0;
                }
                MouseEventKind::Down(_) if mouse.row < 3 && mouse.column < 48 => {
                    app.selecting_service = true;
                }
                MouseEventKind::Down(_) if mouse.column < SIDEBAR_WIDTH && mouse.row >= 5 => {
                    let index = usize::from(mouse.row.saturating_sub(5));
                    if let Some(page) = Page::ALL.get(index).copied() {
                        app.select_page(page);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}

fn handle_key(
    key: KeyEvent,
    runtime: &tokio::runtime::Runtime,
    session: &mut ConsoleSession,
    app: &mut App,
) -> bool {
    if app.query_mode {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => app.query_mode = false,
            KeyCode::Backspace => {
                app.query.pop();
                app.set_cursor(0);
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.query.push(character);
                app.set_cursor(0);
            }
            _ => {}
        }
        return false;
    }
    if app.confirmation.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let confirmation = app
                    .confirmation
                    .take()
                    .unwrap_or(Confirmation::ClearExchanges);
                if let Err(error) = perform_confirmation(confirmation, runtime, session, app) {
                    app.set_notice(error.to_string(), RED);
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => app.confirmation = None,
            _ => {}
        }
        return false;
    }
    if app.show_help {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter) {
            app.show_help = false;
        }
        return false;
    }
    if app.selecting_service {
        match key.code {
            KeyCode::Esc | KeyCode::Char('s') => app.selecting_service = false,
            KeyCode::Down | KeyCode::Char('j') => {
                app.service_cursor = (app.service_cursor + 1)
                    .min(session.catalog().services.len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.service_cursor = app.service_cursor.saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(service) = session.catalog().services.get(app.service_cursor) {
                    let name = service.name.clone();
                    match runtime.block_on(session.select(&name)) {
                        Ok(()) => {
                            let next = runtime.block_on(refresh_runtime(session));
                            app.observations_ready = false;
                            app.known_exchange_ids.clear();
                            apply_runtime_refresh(app, next, false);
                            app.refreshed_at = Instant::now();
                            app.selecting_service = false;
                            app.set_notice(format!("Switched to {name}"), GREEN);
                        }
                        Err(error) => app.set_notice(error.to_string(), RED),
                    }
                }
            }
            _ => {}
        }
        return false;
    }

    match key.code {
        KeyCode::Char('q') => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Char('?') => app.show_help = true,
        KeyCode::Char(character @ '1'..='9') => {
            if let Some(page) = Page::numbered(character) {
                app.select_page(page);
                app.focus = Focus::List;
            }
        }
        KeyCode::Char('R') => {
            app.select_page(Page::Reports);
            app.focus = Focus::List;
        }
        KeyCode::Char('o') => {
            app.select_page(Page::Operations);
            app.focus = Focus::List;
        }
        KeyCode::Char('s') => app.selecting_service = true,
        KeyCode::Char('g') => {
            app.log_dock = app.log_dock.toggle();
            app.focus = if app.log_dock.is_expanded() {
                Focus::Logs
            } else {
                Focus::Navigation
            };
            app.log_scroll = 0;
        }
        KeyCode::Char('G') => {
            app.log_dock = app.log_dock.toggle_full();
            app.focus = Focus::Logs;
            app.log_scroll = 0;
        }
        KeyCode::Char('d') => {
            app.detail_mode = app.detail_mode.toggle();
            app.set_notice(
                if app.detail_mode == DetailMode::Expanded {
                    "Detail pane widened"
                } else {
                    "Detail pane restored"
                },
                GREEN,
            );
        }
        KeyCode::Char('D') => {
            app.detail_mode = app.detail_mode.toggle_full();
            if app.detail_mode == DetailMode::Full {
                app.focus = Focus::Detail;
            }
            app.set_notice(
                if app.detail_mode == DetailMode::Full {
                    "Detail view full screen"
                } else {
                    "Detail view restored"
                },
                GREEN,
            );
        }
        KeyCode::Char('n') => {
            app.sound_enabled = !app.sound_enabled;
            app.set_notice(
                if app.sound_enabled {
                    "Issue sounds enabled"
                } else {
                    "Issue sounds muted"
                },
                if app.sound_enabled { GREEN } else { MUTED },
            );
            if app.sound_enabled {
                ring_terminal();
            }
        }
        KeyCode::Char('/') => {
            app.query_mode = true;
            app.query.clear();
            app.set_cursor(0);
        }
        KeyCode::Char('r') => {
            let next = runtime.block_on(refresh_runtime(session));
            if apply_runtime_refresh(app, next, true) {
                ring_terminal();
            }
            app.refreshed_at = Instant::now();
            if app.notice().is_none() {
                app.set_notice("Runtime refreshed", GREEN);
            }
        }
        KeyCode::Char('a') => {
            app.auto_refresh = !app.auto_refresh;
            app.set_notice(
                if app.auto_refresh {
                    "Live refresh enabled"
                } else {
                    "Live refresh paused"
                },
                if app.auto_refresh { GREEN } else { AMBER },
            );
        }
        KeyCode::Char('x') if app.page == Page::Exchanges && app.runtime.connected() => {
            app.confirmation = Some(Confirmation::ClearExchanges);
        }
        KeyCode::Char('x') if app.page == Page::State && app.runtime.connected() => {
            app.confirmation = Some(Confirmation::ResetState);
        }
        KeyCode::Char('p') if app.page == Page::Exchanges => {
            if let Err(error) = prepare_selected_report(runtime, session, app) {
                app.set_notice(error.to_string(), RED);
            }
        }
        KeyCode::Esc => {
            if app.log_dock == LogDock::Full {
                app.log_dock = LogDock::Partial;
                app.focus = Focus::Logs;
            } else if app.detail_mode == DetailMode::Full {
                app.detail_mode = DetailMode::Expanded;
                app.focus = Focus::Detail;
            } else if app.focus == Focus::Logs {
                app.focus = Focus::Navigation;
            } else if app.page == Page::Operations && app.expanded_outcome.is_some() {
                app.expanded_outcome = None;
            } else if app.page == Page::Settings && app.settings_document.is_some() {
                app.settings_document = None;
                app.focus = Focus::List;
                app.detail_scroll = 0;
            } else if !app.query.is_empty() {
                app.query.clear();
                app.set_cursor(0);
            } else {
                app.focus = Focus::Navigation;
            }
        }
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::Navigation => Focus::List,
                Focus::List => Focus::Detail,
                Focus::Detail if app.log_dock.is_expanded() => Focus::Logs,
                Focus::Detail | Focus::Logs => Focus::Navigation,
            };
        }
        KeyCode::BackTab => {
            app.focus = match app.focus {
                Focus::Navigation if app.log_dock.is_expanded() => Focus::Logs,
                Focus::Navigation => Focus::Detail,
                Focus::List => Focus::Navigation,
                Focus::Detail => Focus::List,
                Focus::Logs => Focus::Detail,
            };
        }
        KeyCode::Enter => match app.focus {
            Focus::Navigation => app.focus = Focus::List,
            Focus::List if app.page == Page::Settings => {
                open_settings_document(app);
                app.focus = Focus::Detail;
                app.detail_scroll = 0;
            }
            Focus::List => app.focus = Focus::Detail,
            Focus::Detail if app.page == Page::Operations => {
                toggle_selected_outcome(app, session.selected());
            }
            Focus::Detail if app.page == Page::Settings => {
                app.settings_document = None;
                app.detail_mode = app.detail_mode.restore_index();
                app.focus = Focus::List;
                app.detail_scroll = 0;
            }
            Focus::Detail => {
                app.detail_mode = app.detail_mode.restore_index();
                app.focus = Focus::List;
            }
            Focus::Logs => {}
        },
        KeyCode::Right | KeyCode::Char('l')
            if app.page == Page::Operations && app.focus == Focus::Detail =>
        {
            expand_selected_outcome(app, session.selected());
        }
        KeyCode::Left | KeyCode::Char('h')
            if app.page == Page::Operations && app.focus == Focus::Detail =>
        {
            if app.expanded_outcome.take().is_none() {
                app.detail_mode = app.detail_mode.restore_index();
                app.focus = Focus::List;
            }
        }
        KeyCode::Right | KeyCode::Char('l')
            if app.page == Page::Settings && app.focus == Focus::List =>
        {
            open_settings_document(app);
            app.focus = Focus::Detail;
            app.detail_scroll = 0;
        }
        KeyCode::Left | KeyCode::Char('h')
            if app.page == Page::Settings && app.focus == Focus::Detail =>
        {
            app.settings_document = None;
            app.detail_mode = app.detail_mode.restore_index();
            app.focus = Focus::List;
            app.detail_scroll = 0;
        }
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::List => {
            app.focus = Focus::Detail;
            app.detail_scroll = 0;
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::Detail => {
            app.detail_mode = app.detail_mode.restore_index();
            app.focus = Focus::List;
            app.detail_scroll = 0;
        }
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::Navigation => app.cycle_page(-1),
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::Navigation => app.cycle_page(1),
        KeyCode::Down | KeyCode::Char('j') => move_vertical(app, session.selected(), 1),
        KeyCode::Up | KeyCode::Char('k') => move_vertical(app, session.selected(), -1),
        KeyCode::PageDown => match app.focus {
            Focus::Detail => app.detail_scroll = app.detail_scroll.saturating_add(10),
            Focus::Logs => app.log_scroll = app.log_scroll.saturating_sub(10),
            Focus::Navigation | Focus::List => move_vertical(app, session.selected(), 10),
        },
        KeyCode::PageUp => match app.focus {
            Focus::Detail => app.detail_scroll = app.detail_scroll.saturating_sub(10),
            Focus::Logs => app.log_scroll = app.log_scroll.saturating_add(10),
            Focus::Navigation | Focus::List => move_vertical(app, session.selected(), -10),
        },
        KeyCode::Home => match app.focus {
            Focus::Detail => app.detail_scroll = 0,
            Focus::Logs => app.log_scroll = usize::MAX,
            Focus::Navigation | Focus::List => app.set_cursor(0),
        },
        KeyCode::End if app.focus == Focus::Logs => app.log_scroll = 0,
        _ => {}
    }
    false
}

fn move_vertical(app: &mut App, service: &ops::ConsoleService, delta: isize) {
    match app.focus {
        Focus::Navigation => {
            let next = app.page.index().saturating_add_signed(delta);
            app.select_page(Page::ALL[next.min(Page::ALL.len() - 1)]);
        }
        Focus::List => app.move_cursor(delta, page_item_count(app, service)),
        Focus::Detail if app.page == Page::Operations => {
            let operations = filtered_operations(service, &app.query);
            let count =
                selected(&operations, app.cursor()).map_or(0, |operation| operation.outcomes.len());
            if count == 0 {
                app.operation_outcome_cursor = 0;
            } else {
                app.operation_outcome_cursor = app
                    .operation_outcome_cursor
                    .min(count - 1)
                    .saturating_add_signed(delta)
                    .min(count - 1);
            }
            app.expanded_outcome = None;
            // Why: outcome navigation is viewport-relative. Applying scroll from the outcome's
            // ordinal alone hides the operation heading even when the row is already visible.
            app.detail_scroll = 0;
        }
        Focus::Detail => {
            let delta = i16::try_from(delta).unwrap_or(if delta.is_negative() {
                i16::MIN
            } else {
                i16::MAX
            });
            app.detail_scroll = app.detail_scroll.saturating_add_signed(delta);
        }
        Focus::Logs => {
            if delta.is_negative() {
                app.log_scroll = app.log_scroll.saturating_add(delta.unsigned_abs());
            } else {
                app.log_scroll = app.log_scroll.saturating_sub(delta.unsigned_abs());
            }
        }
    }
}

fn toggle_selected_outcome(app: &mut App, service: &ops::ConsoleService) {
    let operations = filtered_operations(service, &app.query);
    let count = selected(&operations, app.cursor()).map_or(0, |operation| operation.outcomes.len());
    if count == 0 {
        return;
    }
    let selected = app.operation_outcome_cursor.min(count - 1);
    app.expanded_outcome = (app.expanded_outcome != Some(selected)).then_some(selected);
}

fn expand_selected_outcome(app: &mut App, service: &ops::ConsoleService) {
    let operations = filtered_operations(service, &app.query);
    let count = selected(&operations, app.cursor()).map_or(0, |operation| operation.outcomes.len());
    if count == 0 {
        return;
    }
    let selected = app.operation_outcome_cursor.min(count - 1);
    app.expanded_outcome = Some(selected);
}

fn open_settings_document(app: &mut App) {
    app.settings_document = Some(if app.cursor() == 0 {
        SettingsDocument::Simfile
    } else {
        SettingsDocument::Contract
    });
}

fn page_item_count(app: &App, service: &ops::ConsoleService) -> usize {
    match app.page {
        Page::Service => 1,
        Page::Operations => filtered_operations(service, &app.query).len(),
        Page::Exchanges => filtered_exchanges(&app.runtime.exchanges.exchanges, &app.query).len(),
        Page::State => filtered_resources(&app.runtime.state.resources, &app.query).len(),
        Page::Deferred => {
            if app.runtime.connected() {
                filtered_deferred(&app.runtime.deferred.items, &app.query).len()
            } else {
                service.async_operations.len()
            }
        }
        Page::Scenarios => service.scenarios.len(),
        Page::Faults => service.faults.len(),
        Page::Reports => app.runtime.reports.len(),
        Page::Fidelity => service.findings.len(),
        Page::Settings => 2,
    }
}

fn perform_confirmation(
    confirmation: Confirmation,
    runtime: &tokio::runtime::Runtime,
    session: &ConsoleSession,
    app: &mut App,
) -> Result<()> {
    // Why: retained exchanges and Stack state are evidence. A single accidental keypress must
    // never destroy them, so destructive actions are available only behind an explicit prompt.
    let result = match confirmation {
        Confirmation::ClearExchanges => {
            runtime.block_on(session.act("DELETE", "/.simile/exchanges", None))?
        }
        Confirmation::ResetState => runtime.block_on(session.act(
            "POST",
            "/.simile/state/reset",
            Some(&json!({ "confirm": true })),
        ))?,
    };
    let next = runtime.block_on(refresh_runtime(session));
    apply_runtime_refresh(app, next, false);
    app.refreshed_at = Instant::now();
    app.set_cursor(0);
    let notice = match confirmation {
        Confirmation::ClearExchanges => format!(
            "Cleared {} captured exchanges",
            result
                .get("removed")
                .and_then(Value::as_u64)
                .unwrap_or_default()
        ),
        Confirmation::ResetState => "Local service state reset".to_owned(),
    };
    app.set_notice(notice, GREEN);
    Ok(())
}

fn prepare_selected_report(
    runtime: &tokio::runtime::Runtime,
    session: &ConsoleSession,
    app: &mut App,
) -> Result<()> {
    let exchanges = filtered_exchanges(&app.runtime.exchanges.exchanges, &app.query);
    let Some(exchange) = exchanges.get(app.cursor().min(exchanges.len().saturating_sub(1))) else {
        app.set_notice("Select a captured exchange first", AMBER);
        return Ok(());
    };
    let path = format!("/.simile/exchanges/{}/report", exchange.id);
    let title = format!(
        "{} {} returned {}",
        exchange.method, exchange.path, exchange.status
    );
    let report = runtime.block_on(session.act("POST", &path, Some(&json!({ "title": title }))))?;
    let report_id = report
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("prepared report");
    let next = runtime.block_on(refresh_runtime(session));
    apply_runtime_refresh(app, next, false);
    app.refreshed_at = Instant::now();
    app.set_notice(format!("Verified {report_id}"), GREEN);
    Ok(())
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        drop(disable_raw_mode());
        let mut stdout = io::stdout();
        drop(execute!(stdout, DisableMouseCapture, LeaveAlternateScreen));
    }
}

async fn refresh_runtime(session: &ConsoleSession) -> RuntimeViews {
    let reports = session
        .reports()
        .map(|report| report.reports)
        .unwrap_or_default();
    let (inspection, exchanges, state, deferred) = tokio::join!(
        session.inspect("/.simile/inspection"),
        session.inspect("/.simile/exchanges"),
        session.inspect("/.simile/state"),
        session.inspect("/.simile/deferred"),
    );
    RuntimeViews {
        inspection: inspection.and_then(decode),
        exchanges: exchanges.and_then(decode).unwrap_or_default(),
        state: state.and_then(decode).unwrap_or_default(),
        deferred: deferred.and_then(decode).unwrap_or_default(),
        reports,
    }
}

fn apply_runtime_refresh(app: &mut App, next: RuntimeViews, notify: bool) -> bool {
    let issue = (app.observations_ready && app.runtime.connected()).then(|| {
        next.exchanges
            .exchanges
            .iter()
            .find(|exchange| {
                exchange.status >= 400 && !app.known_exchange_ids.contains(&exchange.id)
            })
            .map(|exchange| {
                (
                    exchange.method.clone(),
                    exchange.path.clone(),
                    exchange.status,
                )
            })
    });
    let disconnected = app.observations_ready && app.runtime.connected() && !next.connected();
    app.known_exchange_ids = next
        .exchanges
        .exchanges
        .iter()
        .map(|exchange| exchange.id.clone())
        .collect();
    app.runtime = next;
    app.observations_ready = true;

    if !notify {
        return false;
    }
    if let Some(Some((method, path, status))) = issue {
        app.set_notice(
            format!("New issue · {method} {path} returned {status}"),
            if status >= 500 { RED } else { AMBER },
        );
        return app.sound_enabled;
    }
    if disconnected {
        app.set_notice("Local service inspection disconnected", RED);
        return app.sound_enabled;
    }
    false
}

fn ring_terminal() {
    // Why: the terminal bell is portable, respects the user's terminal-level sound policy, and
    // does not require a platform-specific audio process or write visible bytes into the TUI.
    let _ = io::stdout().write_all(b"\x07");
    let _ = io::stdout().flush();
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Option<T> {
    serde_json::from_value(value).ok()
}

fn draw(frame: &mut Frame<'_>, session: &ConsoleSession, app: &App) {
    frame.render_widget(
        Block::default().style(Style::default().bg(BACKGROUND).fg(TEXT)),
        frame.area(),
    );
    if frame.area().width < 76 || frame.area().height < 20 {
        draw_too_small(frame);
        return;
    }
    let dock_height = log_dock_height(frame.area().height, app.log_dock);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(dock_height),
            Constraint::Length(2),
        ])
        .split(frame.area());
    draw_topbar(frame, vertical[0], session, app);
    if vertical[1].height > 0 {
        if app.detail_mode == DetailMode::Full && app.focus == Focus::Detail {
            draw_workspace(frame, vertical[1], session.selected(), app);
        } else {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(54)])
                .split(vertical[1]);
            draw_sidebar(frame, body[0], session.selected(), app);
            draw_workspace(frame, body[1], session.selected(), app);
        }
    }
    draw_log_dock(frame, vertical[2], app);
    draw_footer(frame, vertical[3], app);

    if app.selecting_service {
        draw_service_switcher(frame, session, app);
    } else if let Some(confirmation) = app.confirmation {
        draw_confirmation(frame, confirmation);
    } else if app.show_help {
        draw_help(frame);
    }
}

fn log_dock_height(terminal_height: u16, state: LogDock) -> u16 {
    match state {
        LogDock::Collapsed => 3,
        LogDock::Partial => ((terminal_height * 35) / 100)
            .clamp(7, 18)
            .min(terminal_height.saturating_sub(12).max(3)),
        LogDock::Full => terminal_height.saturating_sub(5),
    }
}

fn log_dock_header_row(app: &App) -> Option<u16> {
    let (_, height) = crossterm::terminal::size().ok()?;
    let dock_height = log_dock_height(height, app.log_dock);
    Some(height.saturating_sub(2).saturating_sub(dock_height))
}

fn draw_too_small(frame: &mut Frame<'_>) {
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::styled("Simile console", Style::default().fg(GREEN)),
            Line::raw(""),
            Line::raw("The terminal is too small for the immersive console."),
            Line::styled("Resize to at least 76 × 20.", Style::default().fg(MUTED)),
        ]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(true)),
        ),
        centered_rect(58, 8, frame.area()),
    );
}

fn draw_topbar(frame: &mut Frame<'_>, area: Rect, session: &ConsoleSession, app: &App) {
    let service = &session.selected().service;
    let connected = app.runtime.connected();
    let ownership = if session.owns_runtime() {
        "console owned"
    } else {
        "attached"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ◉ SIMILE ", Style::default().fg(GREEN)),
            Span::styled("│ ", Style::default().fg(BORDER_STRONG)),
            Span::styled(&service.name, Style::default().fg(TEXT)),
            Span::styled(
                format!("  {}  ▾ [s] switch", service.version),
                Style::default().fg(MUTED),
            ),
            Span::raw("   "),
            Span::styled(
                if connected { "●" } else { "○" },
                Style::default().fg(if connected { GREEN } else { MUTED }),
            ),
            Span::styled(
                format!(
                    " Local service · {} · {ownership}",
                    session.runtime_address()
                ),
                Style::default().fg(MUTED),
            ),
            Span::raw("     "),
            Span::styled(
                if app.auto_refresh { "LIVE" } else { "PAUSED" },
                Style::default().fg(if app.auto_refresh { GREEN } else { AMBER }),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(BORDER)),
        ),
        area,
    );
}

fn draw_sidebar(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let mut lines = vec![
        Line::styled("  SIMILE CONSOLE", Style::default().fg(MUTED)),
        Line::raw(""),
    ];
    for page in Page::ALL {
        let active = page == app.page;
        let focused = active && app.focus == Focus::Navigation;
        let count = match page {
            Page::Operations => Some(service.metrics.operations),
            Page::Exchanges => Some(app.runtime.exchanges.exchanges.len()),
            Page::Deferred => Some(app.runtime.deferred.items.len()),
            Page::Scenarios => Some(service.scenarios.len()),
            Page::Faults => Some(service.faults.len()),
            Page::Reports => Some(app.runtime.reports.len()),
            Page::Fidelity => Some(service.findings.len()),
            Page::Service | Page::State | Page::Settings => None,
        };
        let label = format!(
            " {} {} {:<13}{:>3}",
            page.shortcut(),
            page.symbol(),
            page.label(),
            count.map_or_else(String::new, |count| count.to_string()),
        );
        lines.push(Line::styled(
            label,
            if active {
                Style::default()
                    .fg(GREEN)
                    .bg(if focused { GREEN_MUTED } else { SURFACE })
            } else {
                Style::default().fg(MUTED)
            },
        ));
    }
    lines.push(Line::raw(""));
    let reviewed = service
        .metrics
        .outcomes
        .saturating_sub(service.metrics.schema_synthesized_outcomes);
    lines.extend([
        Line::styled("  OUTCOME FIDELITY", Style::default().fg(MUTED)),
        Line::from(vec![
            Span::styled("  ■", Style::default().fg(GREEN)),
            Span::styled(
                format!(" {reviewed}/{} reviewed", service.metrics.outcomes),
                Style::default().fg(TEXT),
            ),
        ]),
        Line::styled(
            format!("  {} warnings", service.metrics.warnings),
            Style::default().fg(if service.metrics.warnings == 0 {
                MUTED
            } else {
                AMBER
            }),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(BORDER)),
        ),
        area,
    );
}

fn draw_workspace(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let area = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(area);
    let title = if app.detail_mode == DetailMode::Full && app.focus == Focus::Detail {
        Line::from(vec![
            Span::styled(
                app.page.label(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  DETAIL FULL SCREEN", Style::default().fg(GREEN)),
            Span::styled("  ·  D restore", Style::default().fg(MUTED)),
        ])
    } else {
        Line::styled(
            app.page.label(),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )
    };
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            title,
            Line::styled(app.page.description(), Style::default().fg(MUTED)),
        ])),
        sections[0],
    );
    match app.page {
        Page::Service => draw_service(frame, sections[1], service, app),
        Page::Operations => draw_operations(frame, sections[1], service, app),
        Page::Exchanges => draw_exchanges(frame, sections[1], service, app),
        Page::State => draw_state(frame, sections[1], app),
        Page::Deferred => draw_deferred(frame, sections[1], service, app),
        Page::Scenarios => draw_scenarios(frame, sections[1], service, app),
        Page::Faults => draw_faults(frame, sections[1], service, app),
        Page::Reports => draw_reports(frame, sections[1], app),
        Page::Fidelity => draw_fidelity(frame, sections[1], service, app),
        Page::Settings => draw_settings(frame, sections[1], service, app),
    }
}

fn draw_service(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(8)])
        .split(area);
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 4); 4])
        .split(rows[0]);
    let runtime_status = app
        .runtime
        .inspection
        .as_ref()
        .map_or("OFFLINE", |inspection| inspection.status.as_str());
    metric_card(
        frame,
        cards[0],
        "Runtime",
        runtime_status,
        app.runtime.connected(),
    );
    metric_card(
        frame,
        cards[1],
        "Operations",
        service.metrics.operations,
        true,
    );
    metric_card(frame, cards[2], "Outcomes", service.metrics.outcomes, true);
    metric_card(
        frame,
        cards[3],
        "Audit warnings",
        service.metrics.warnings,
        service.metrics.warnings == 0,
    );

    let columns = master_detail(rows[1], app.focus, app.detail_mode);
    let inspection = app.runtime.inspection.as_ref();
    let overview = Text::from(vec![
        Line::styled(&service.service.name, Style::default().fg(TEXT)),
        Line::styled(
            service
                .service
                .description
                .as_deref()
                .unwrap_or("Local service emulator"),
            Style::default().fg(MUTED),
        ),
        Line::raw(""),
        fact("Listener", service.service.listen),
        fact(
            "Contract",
            format!("OpenAPI {}", service.service.openapi_version),
        ),
        fact("Profiles", service.metrics.profiles),
        fact("Backends", service.metrics.backends),
        fact("Async operations", service.metrics.async_operations),
        fact("Artifact", short_digest(&service.artifact.sha256)),
    ]);
    frame.render_widget(
        Paragraph::new(overview)
            .block(panel(" Service", app.focus == Focus::List))
            .wrap(Wrap { trim: false }),
        columns[0],
    );
    let runtime = Text::from(vec![
        status_line("Connection", app.runtime.connected()),
        Line::raw(""),
        fact(
            "Captured exchanges",
            inspection.map_or(0, |view| view.exchanges),
        ),
        fact(
            "Stored resources",
            inspection.map_or(0, |view| view.resources),
        ),
        fact("Journal events", inspection.map_or(0, |view| view.events)),
        fact("Deferred pending", app.runtime.deferred.pending),
        fact("Deferred leased", app.runtime.deferred.leased),
        fact("Dead letters", app.runtime.deferred.dead_letters),
        Line::raw(""),
        Line::styled(
            "r refreshes now · a toggles live refresh",
            Style::default().fg(MUTED),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(runtime)
            .block(panel(" Live runtime", app.focus == Focus::Detail))
            .scroll((app.detail_scroll, 0)),
        columns[1],
    );
}

fn draw_operations(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let operations = filtered_operations(service, &app.query);
    let selected = selected(&operations, app.cursor());
    let panes = master_detail(area, app.focus, app.detail_mode);
    let height = usize::from(panes[0].height.saturating_sub(4));
    let (start, end) = visible_window(operations.len(), app.cursor(), height);
    let rows = operations[start..end]
        .iter()
        .enumerate()
        .map(|(offset, operation)| {
            let index = start + offset;
            Row::new(vec![
                Cell::from(operation.method.as_str()).style(method_style(&operation.method)),
                Cell::from(operation.path.as_str()),
                Cell::from(operation.id.as_str()),
                Cell::from(operation.success.to_string()).style(status_style(operation.success)),
            ])
            .style(row_style(index == app.cursor(), app.focus == Focus::List))
        });
    let title = searchable_title(" Operations", operations.len(), app);
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(7),
                Constraint::Percentage(42),
                Constraint::Percentage(42),
                Constraint::Length(5),
            ],
        )
        .header(table_header(["METHOD", "PATH", "OPERATION", "HTTP"]))
        .column_spacing(1)
        .block(panel(&title, app.focus == Focus::List)),
        panes[0],
    );
    let detail_panel = panel(" Contract detail", app.focus == Focus::Detail);
    let detail_area = detail_panel.inner(panes[1]);
    frame.render_widget(detail_panel, panes[1]);
    let Some(operation) = selected else {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No matching operations",
                Style::default().fg(MUTED),
            )),
            detail_area,
        );
        return;
    };

    // Why: the method, path, and operation id establish context for everything below them.
    // Keeping that identity fixed prevents list navigation from turning detail inspection into
    // an orientation task.
    let detail_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(detail_area);
    frame.render_widget(
        Paragraph::new(operation_heading(operation)).wrap(Wrap { trim: false }),
        detail_sections[0],
    );
    let body_text = operation_detail(operation, app);
    let body_width = usize::from(detail_sections[1].width);
    let total_rows = wrapped_text_height(&body_text, body_width);
    let selected_line = selected_outcome_line(operation, app);
    let selected_row = wrapped_lines_height(&body_text.lines[..selected_line], body_width);
    let body = Paragraph::new(body_text).wrap(Wrap { trim: false });
    let scroll = viewport_scroll(
        app.detail_scroll,
        selected_row,
        total_rows,
        usize::from(detail_sections[1].height),
    );
    frame.render_widget(body.scroll((scroll, 0)), detail_sections[1]);
}

fn operation_heading(operation: &ops::ConsoleOperation) -> Text<'_> {
    Text::from(vec![
        Line::from(vec![
            Span::styled(&operation.method, method_style(&operation.method)),
            Span::raw("  "),
            Span::styled(&operation.path, Style::default().fg(TEXT)),
        ]),
        Line::styled(&operation.id, Style::default().fg(CYAN)),
        fact("Outcomes", operation.outcomes.len()),
    ])
}

fn operation_detail<'a>(operation: &'a ops::ConsoleOperation, app: &'a App) -> Text<'a> {
    let mut lines = vec![Line::styled("OUTCOMES", Style::default().fg(MUTED))];
    for (index, outcome) in operation.outcomes.iter().enumerate() {
        let expanded = app.expanded_outcome == Some(index);
        let selected = app.operation_outcome_cursor == index;
        lines.push(
            Line::from(vec![
                Span::styled(
                    if expanded { "▾ " } else { "▸ " },
                    Style::default().fg(MUTED),
                ),
                Span::styled(
                    format!("{:>3}", outcome.status),
                    status_style(outcome.status),
                ),
                Span::styled(format!("  {:<24}", outcome.name), Style::default().fg(TEXT)),
                Span::styled(
                    provenance_label(&outcome.provenance),
                    Style::default().fg(MUTED),
                ),
            ])
            .style(row_style(selected, app.focus == Focus::Detail)),
        );
        if expanded {
            lines.push(Line::styled("", Style::default().bg(SURFACE)));
            lines.extend(outcome_detail(outcome));
            lines.push(Line::styled("", Style::default().bg(SURFACE)));
        }
    }
    if !operation.outcomes.is_empty() {
        lines.push(Line::styled(
            "j/k select · enter or → expand · ← collapse",
            Style::default().fg(MUTED),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(&operation.summary, Style::default().fg(TEXT)));
    if operation.description != operation.summary {
        lines.push(Line::styled(
            &operation.description,
            Style::default().fg(MUTED),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(fact(
        "Security",
        if operation.secured {
            "required"
        } else {
            "public"
        },
    ));
    lines.push(fact("Behavior", &operation.behavior_source));
    lines.push(fact("Request", &operation.request_schema));
    lines.push(fact("Response", &operation.response_schema));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "REPRESENTATIVE REQUEST",
        Style::default().fg(MUTED),
    ));
    lines.extend(json_lines(&operation.request_example, 12));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "REPRESENTATIVE RESPONSE",
        Style::default().fg(MUTED),
    ));
    lines.extend(json_lines(&operation.response_example, 12));
    Text::from(lines)
}

fn selected_outcome_line(operation: &ops::ConsoleOperation, app: &App) -> usize {
    if operation.outcomes.is_empty() {
        return 0;
    }
    let selected = app
        .operation_outcome_cursor
        .min(operation.outcomes.len() - 1);
    let mut line = 1;
    for (index, outcome) in operation.outcomes.iter().enumerate().take(selected) {
        line += 1;
        if app.expanded_outcome == Some(index) {
            line += outcome_detail(outcome).len() + 2;
        }
    }
    line
}

fn viewport_scroll(
    requested: u16,
    selected_row: usize,
    total_rows: usize,
    visible_rows: usize,
) -> u16 {
    if visible_rows == 0 {
        return 0;
    }
    let maximum = total_rows.saturating_sub(visible_rows);
    let reveal = selected_row.saturating_add(1).saturating_sub(visible_rows);
    u16::try_from(usize::from(requested).max(reveal).min(maximum)).unwrap_or(u16::MAX)
}

fn wrapped_text_height(text: &Text<'_>, width: usize) -> usize {
    wrapped_lines_height(&text.lines, width)
}

fn wrapped_lines_height(lines: &[Line<'_>], width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

fn outcome_detail(outcome: &ops::ConsoleOutcome) -> Vec<Line<'_>> {
    let mut lines = vec![
        fact("  Status", outcome.status),
        fact("  Provenance", provenance_label(&outcome.provenance)),
        fact(
            "  Content type",
            outcome.content_type.as_deref().unwrap_or("not declared"),
        ),
        fact("  Response schema", &outcome.response_schema),
        fact("  Schema variants", outcome.response_variants),
    ];
    if !outcome.headers.is_empty() {
        lines.push(fact("  Headers", outcome.headers.join(", ")));
    }
    if !outcome.description.is_empty() {
        lines.push(Line::styled(
            format!("  {}", outcome.description),
            Style::default().fg(MUTED),
        ));
    }
    if let Some(example) = &outcome.example {
        lines.push(Line::styled("  EXAMPLE", Style::default().fg(MUTED)));
        lines.extend(json_lines(example, 18).into_iter().map(|line| {
            let mut spans = vec![Span::raw("  ")];
            spans.extend(line.spans);
            Line::from(spans)
        }));
    }
    lines
        .into_iter()
        .map(|line| {
            let mut spans = vec![Span::styled("  │ ", Style::default().fg(BLUE))];
            spans.extend(line.spans);
            Line::from(spans).style(Style::default().bg(SURFACE))
        })
        .collect()
}

fn draw_exchanges(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let exchanges = filtered_exchanges(&app.runtime.exchanges.exchanges, &app.query);
    let selected = selected(&exchanges, app.cursor());
    let panes = master_detail(area, app.focus, app.detail_mode);
    let height = usize::from(panes[0].height.saturating_sub(4));
    let (start, end) = visible_window(exchanges.len(), app.cursor(), height);
    let rows = exchanges[start..end]
        .iter()
        .enumerate()
        .map(|(offset, exchange)| {
            let index = start + offset;
            Row::new(vec![
                Cell::from(format_clock(exchange.started_at_ms)).style(Style::default().fg(MUTED)),
                Cell::from(exchange.method.as_str()).style(method_style(&exchange.method)),
                Cell::from(exchange.path.as_str()),
                Cell::from(exchange.status.to_string()).style(status_style(exchange.status)),
                Cell::from(format_duration(exchange.duration_micros)),
            ])
            .style(row_style(index == app.cursor(), app.focus == Focus::List))
        });
    let mut title = searchable_title(" Exchanges", exchanges.len(), app);
    if app.runtime.connected() {
        title.push_str(" · x clear · p report");
    }
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(9),
                Constraint::Length(7),
                Constraint::Percentage(55),
                Constraint::Length(5),
                Constraint::Length(8),
            ],
        )
        .header(table_header([
            "RECEIVED", "METHOD", "PATH", "HTTP", "LATENCY",
        ]))
        .column_spacing(1)
        .block(panel(&title, app.focus == Focus::List)),
        panes[0],
    );
    let detail = selected.map_or_else(
        || {
            Text::from(vec![
                Line::styled("No captured exchanges", Style::default().fg(TEXT)),
                Line::styled(
                    format!("Send a request to http://{}", service.service.listen),
                    Style::default().fg(MUTED),
                ),
            ])
        },
        |exchange| exchange_detail(exchange, service),
    );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel(" Exchange detail", app.focus == Focus::Detail))
            .scroll((app.detail_scroll, 0))
            .wrap(Wrap { trim: false }),
        panes[1],
    );
}

fn exchange_detail<'a>(exchange: &'a Exchange, service: &'a ops::ConsoleService) -> Text<'a> {
    let operation_id = service
        .operations
        .iter()
        .find(|operation| operation.key == exchange.operation)
        .map_or(exchange.operation.as_str(), |operation| {
            operation.id.as_str()
        });
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                if exchange.status < 400 {
                    "DELIVERED"
                } else {
                    "ISSUE"
                },
                Style::default().fg(if exchange.status < 400 { GREEN } else { AMBER }),
            ),
            Span::raw("  "),
            Span::styled(&exchange.id, Style::default().fg(MUTED)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(&exchange.method, method_style(&exchange.method)),
            Span::raw("  "),
            Span::styled(&exchange.path, Style::default().fg(TEXT)),
        ]),
        fact("Operation", operation_id),
        fact("Contract key", &exchange.operation),
        fact("Response", exchange.status),
        fact("Duration", format_duration(exchange.duration_micros)),
        fact("Outcome", exchange.outcome.as_deref().unwrap_or("—")),
        fact("Profile", exchange.profile.as_deref().unwrap_or("—")),
        Line::raw(""),
        Line::styled("REQUEST", Style::default().fg(MUTED)),
        message_line(&exchange.request),
    ];
    lines.extend(message_body_lines(&exchange.request, 16));
    lines.push(Line::raw(""));
    lines.push(Line::styled("RESPONSE", Style::default().fg(MUTED)));
    lines.push(message_line(&exchange.response));
    lines.extend(message_body_lines(&exchange.response, 16));
    Text::from(lines)
}

fn draw_log_dock(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let logs = runtime_logs(&app.runtime);
    let visible_height = usize::from(area.height.saturating_sub(2)).max(1);
    let (start, end) = log_window(logs.len(), visible_height, app.log_scroll);
    let mut lines = logs[start..end]
        .iter()
        .rev()
        .map(log_line)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(" ▎ ", Style::default().fg(BORDER_STRONG)),
            Span::styled(
                "Waiting for HTTP and Stack activity from this local service…",
                Style::default().fg(MUTED),
            ),
        ]));
    }

    let mode = match app.log_dock {
        LogDock::Collapsed => "[g] open  [G] full",
        LogDock::Partial => "[g] close  [G] full",
        LogDock::Full => "FULL  [g] close  [G] restore",
    };
    let live = if app.auto_refresh {
        "● LIVE"
    } else {
        "○ PAUSED"
    };
    let title = Line::from(vec![
        Span::styled(" LIVE LOGS ", Style::default().fg(TEXT)),
        Span::styled(
            format!("{} entries  ", logs.len()),
            Style::default().fg(MUTED),
        ),
        Span::styled(
            format!("{live}  "),
            Style::default().fg(if app.auto_refresh { GREEN } else { AMBER }),
        ),
        Span::styled(mode, Style::default().fg(MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style(app.focus == Focus::Logs)),
        ),
        area,
    );
}

fn log_window(length: usize, visible: usize, scroll: usize) -> (usize, usize) {
    let start = scroll.min(length.saturating_sub(visible));
    (start, start.saturating_add(visible).min(length))
}

fn log_line(entry: &LogEntry) -> Line<'_> {
    let (symbol, status_style) = match entry.level {
        "ERROR" => ("×", Style::default().fg(RED)),
        "WARN" => ("!", Style::default().fg(AMBER)),
        _ => ("✓", Style::default().fg(GREEN)),
    };
    Line::from(vec![
        Span::styled(" ▎ ", Style::default().fg(BLUE)),
        Span::styled(
            format_log_timestamp(entry.timestamp_ms),
            Style::default().fg(MUTED),
        ),
        Span::raw("  "),
        Span::styled(symbol, status_style),
        Span::raw("  "),
        Span::styled(format!("{:<5}", entry.source), Style::default().fg(CYAN)),
        Span::raw("  "),
        Span::styled(&entry.summary, Style::default().fg(TEXT)),
        Span::styled(format!("  {}", entry.context), Style::default().fg(MUTED)),
    ])
}

fn draw_state(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let resources = filtered_resources(&app.runtime.state.resources, &app.query);
    let selected = selected(&resources, app.cursor());
    let panes = master_detail(area, app.focus, app.detail_mode);
    let height = usize::from(panes[0].height.saturating_sub(4));
    let (start, end) = visible_window(resources.len(), app.cursor(), height);
    let rows = resources[start..end]
        .iter()
        .enumerate()
        .map(|(offset, resource)| {
            let index = start + offset;
            Row::new(vec![
                Cell::from(resource.address.kind.as_str()).style(Style::default().fg(CYAN)),
                Cell::from(resource.address.name.as_str()),
                Cell::from(if resource.value.is_some() {
                    "stored"
                } else {
                    "empty"
                })
                .style(Style::default().fg(if resource.value.is_some() {
                    GREEN
                } else {
                    MUTED
                })),
            ])
            .style(row_style(index == app.cursor(), app.focus == Focus::List))
        });
    let mut title = searchable_title(" Resources", resources.len(), app);
    if app.runtime.connected() {
        title.push_str(" · x reset");
    }
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Percentage(65),
                Constraint::Length(8),
            ],
        )
        .header(table_header(["KIND", "RESOURCE", "STATE"]))
        .column_spacing(1)
        .block(panel(&title, app.focus == Focus::List)),
        panes[0],
    );
    let detail = selected.map_or_else(
        || {
            Text::from(Line::styled(
                "No Stack resources",
                Style::default().fg(MUTED),
            ))
        },
        |resource| {
            let mut lines = vec![
                Line::styled(
                    format!("{}.{}", resource.address.kind, resource.address.name),
                    Style::default().fg(TEXT),
                ),
                Line::styled(
                    format!(
                        "{} journal events in this runtime",
                        app.runtime.state.events.len()
                    ),
                    Style::default().fg(MUTED),
                ),
                Line::raw(""),
                Line::styled("CURRENT RESOURCE STATE", Style::default().fg(MUTED)),
            ];
            lines.extend(json_lines(
                resource.value.as_ref().unwrap_or(&Value::Null),
                usize::MAX,
            ));
            Text::from(lines)
        },
    );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel(" Resource detail", app.focus == Focus::Detail))
            .scroll((app.detail_scroll, 0))
            .wrap(Wrap { trim: false }),
        panes[1],
    );
}

fn draw_deferred(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    if !app.runtime.connected() {
        draw_declared_async(frame, area, service, app);
        return;
    }
    let items = filtered_deferred(&app.runtime.deferred.items, &app.query);
    let selected = selected(&items, app.cursor());
    let panes = master_detail(area, app.focus, app.detail_mode);
    let height = usize::from(panes[0].height.saturating_sub(4));
    let (start, end) = visible_window(items.len(), app.cursor(), height);
    let rows = items[start..end].iter().enumerate().map(|(offset, item)| {
        let index = start + offset;
        let name = value_string(&item.work, "name").unwrap_or("—");
        Row::new(vec![
            Cell::from(item.id.as_str()),
            Cell::from(name).style(Style::default().fg(CYAN)),
            Cell::from(item.receive_count.to_string()),
            Cell::from(deferred_status(item)).style(Style::default().fg(deferred_color(item))),
        ])
        .style(row_style(index == app.cursor(), app.focus == Focus::List))
    });
    let title = format!(
        " Queue · {} pending · {} leased · {} failed",
        app.runtime.deferred.pending,
        app.runtime.deferred.leased,
        app.runtime.deferred.dead_letters
    );
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(45),
                Constraint::Percentage(35),
                Constraint::Length(8),
                Constraint::Length(9),
            ],
        )
        .header(table_header(["MESSAGE", "WORK", "ATTEMPTS", "STATE"]))
        .column_spacing(1)
        .block(panel(&title, app.focus == Focus::List)),
        panes[0],
    );
    let detail = selected.map_or_else(
        || {
            Text::from(Line::styled(
                "No runtime work is queued",
                Style::default().fg(MUTED),
            ))
        },
        |item| {
            let mut lines = vec![
                Line::styled(&item.id, Style::default().fg(TEXT)),
                fact("Queue", &item.queue),
                fact("State", deferred_status(item)),
                fact("Attempts", item.receive_count),
                Line::raw(""),
                Line::styled("WORK", Style::default().fg(MUTED)),
            ];
            lines.extend(json_lines(&item.work, usize::MAX));
            Text::from(lines)
        },
    );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel(" Work detail", app.focus == Focus::Detail))
            .scroll((app.detail_scroll, 0))
            .wrap(Wrap { trim: false }),
        panes[1],
    );
}

fn draw_declared_async(
    frame: &mut Frame<'_>,
    area: Rect,
    service: &ops::ConsoleService,
    app: &App,
) {
    let panes = master_detail(area, app.focus, app.detail_mode);
    let rows = service
        .async_operations
        .iter()
        .enumerate()
        .map(|(index, operation)| {
            Row::new(vec![
                Cell::from(operation.name.as_str()).style(Style::default().fg(CYAN)),
                Cell::from(operation.backends.len().to_string()),
                Cell::from("DECLARED").style(Style::default().fg(MUTED)),
            ])
            .style(row_style(index == app.cursor(), app.focus == Focus::List))
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(65),
                Constraint::Length(10),
                Constraint::Length(10),
            ],
        )
        .header(table_header(["OPERATION", "BACKENDS", "STATE"]))
        .block(panel(
            " Declared work · runtime offline",
            app.focus == Focus::List,
        )),
        panes[0],
    );
    let detail = service
        .async_operations
        .get(
            app.cursor()
                .min(service.async_operations.len().saturating_sub(1)),
        )
        .map_or_else(
            || {
                Text::from(Line::styled(
                    "No asynchronous work declared",
                    Style::default().fg(MUTED),
                ))
            },
            |operation| {
                Text::from(vec![
                    Line::styled(&operation.name, Style::default().fg(TEXT)),
                    Line::styled(
                        operation.description.as_deref().unwrap_or("No description"),
                        Style::default().fg(MUTED),
                    ),
                    Line::raw(""),
                    fact("Backends", operation.backends.join(", ")),
                ])
            },
        );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel(" Work declaration", app.focus == Focus::Detail))
            .wrap(Wrap { trim: false }),
        panes[1],
    );
}

fn draw_scenarios(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let panes = master_detail(area, app.focus, app.detail_mode);
    let height = usize::from(panes[0].height.saturating_sub(3));
    let (start, end) = visible_window(service.scenarios.len(), app.cursor(), height);
    let rows = service.scenarios[start..end]
        .iter()
        .enumerate()
        .map(|(offset, scenario)| {
            let index = start + offset;
            Row::new(vec![
                Cell::from(scenario.name.as_str()).style(Style::default().fg(CYAN)),
                Cell::from(scenario.selections.to_string()),
                Cell::from(scenario.faults.to_string()),
            ])
            .style(row_style(index == app.cursor(), app.focus == Focus::List))
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(65),
                Constraint::Length(12),
                Constraint::Length(8),
            ],
        )
        .header(table_header(["SCENARIO", "SELECTIONS", "FAULTS"]))
        .block(panel(
            &format!(" Scenarios · {}", service.scenarios.len()),
            app.focus == Focus::List,
        )),
        panes[0],
    );
    let detail = service
        .scenarios
        .get(app.cursor().min(service.scenarios.len().saturating_sub(1)))
        .map_or_else(
            || {
                Text::from(Line::styled(
                    "No deterministic scenarios",
                    Style::default().fg(MUTED),
                ))
            },
            |scenario| {
                Text::from(vec![
                    Line::styled(&scenario.name, Style::default().fg(TEXT)),
                    Line::styled(
                        scenario.description.as_deref().unwrap_or("No description"),
                        Style::default().fg(MUTED),
                    ),
                    Line::raw(""),
                    fact("Outcome selections", scenario.selections),
                    fact("Fault injections", scenario.faults),
                    Line::raw(""),
                    Line::styled("WHEN", Style::default().fg(MUTED)),
                    Line::styled(
                        format!("X-Simile-Scenario: {}", scenario.name),
                        Style::default().fg(CYAN),
                    ),
                    Line::raw(""),
                    Line::styled("THEN", Style::default().fg(MUTED)),
                    Line::raw("Activate the compiled selections and faults."),
                ])
            },
        );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel(" Scenario detail", app.focus == Focus::Detail))
            .scroll((app.detail_scroll, 0))
            .wrap(Wrap { trim: false }),
        panes[1],
    );
}

fn draw_faults(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let panes = master_detail(area, app.focus, app.detail_mode);
    let rows = service.faults.iter().enumerate().map(|(index, fault)| {
        Row::new(vec![
            Cell::from(fault.name.as_str()).style(Style::default().fg(AMBER)),
            Cell::from(fault.phase.as_str()),
            Cell::from(fault.action.as_str()),
        ])
        .style(row_style(index == app.cursor(), app.focus == Focus::List))
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(52),
                Constraint::Length(13),
                Constraint::Percentage(35),
            ],
        )
        .header(table_header(["FAULT", "PHASE", "ACTION"]))
        .block(panel(
            &format!(
                " Faults · {} definitions · {} profiles",
                service.faults.len(),
                service.fault_profiles.len()
            ),
            app.focus == Focus::List,
        )),
        panes[0],
    );
    let detail = service
        .faults
        .get(app.cursor().min(service.faults.len().saturating_sub(1)))
        .map_or_else(
            || {
                Text::from(Line::styled(
                    "No fault definitions",
                    Style::default().fg(MUTED),
                ))
            },
            |fault| {
                let profiles = service
                    .fault_profiles
                    .iter()
                    .filter(|profile| profile.faults.contains(&fault.name))
                    .map(|profile| profile.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Text::from(vec![
                    Line::styled(&fault.name, Style::default().fg(AMBER)),
                    Line::styled(
                        fault.description.as_deref().unwrap_or("No description"),
                        Style::default().fg(MUTED),
                    ),
                    Line::raw(""),
                    fact("Phase", &fault.phase),
                    fact("Action", &fault.action),
                    fact(
                        "Profiles",
                        if profiles.is_empty() {
                            "available"
                        } else {
                            &profiles
                        },
                    ),
                ])
            },
        );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel(" Fault detail", app.focus == Focus::Detail))
            .scroll((app.detail_scroll, 0))
            .wrap(Wrap { trim: false }),
        panes[1],
    );
}

fn draw_reports(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let panes = master_detail(area, app.focus, app.detail_mode);
    let height = usize::from(panes[0].height.saturating_sub(4));
    let (start, end) = visible_window(app.runtime.reports.len(), app.cursor(), height);
    let rows = app.runtime.reports[start..end]
        .iter()
        .enumerate()
        .map(|(offset, report)| {
            let index = start + offset;
            Row::new(vec![
                Cell::from(report.created_at.as_str()),
                Cell::from(report.status.display_name()).style(report_status_style(report)),
                Cell::from(report.operation.as_str()).style(Style::default().fg(CYAN)),
                Cell::from(report.title.as_str()),
            ])
            .style(row_style(index == app.cursor(), app.focus == Focus::List))
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(21),
                Constraint::Length(20),
                Constraint::Percentage(30),
                Constraint::Percentage(45),
            ],
        )
        .header(table_header(["CREATED", "STATUS", "OPERATION", "TITLE"]))
        .column_spacing(1)
        .block(panel(
            &format!(" Reports · {} prepared", app.runtime.reports.len()),
            app.focus == Focus::List,
        )),
        panes[0],
    );
    let detail = app
        .runtime
        .reports
        .get(
            app.cursor()
                .min(app.runtime.reports.len().saturating_sub(1)),
        )
        .map_or_else(
            || {
                Text::from(vec![
                    Line::styled("No reports prepared", Style::default().fg(MUTED)),
                    Line::raw(""),
                    Line::raw("Select an exchange and press p to prepare a verified report."),
                ])
            },
            report_detail,
        );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel(" Report detail", app.focus == Focus::Detail))
            .scroll((app.detail_scroll, 0))
            .wrap(Wrap { trim: false }),
        panes[1],
    );
}

fn report_detail(report: &ReportDocument) -> Text<'_> {
    let mut lines = vec![
        Line::styled(&report.title, Style::default().fg(TEXT)),
        Line::styled(&report.id, Style::default().fg(CYAN)),
        Line::raw(""),
        fact("Status", report.status.display_name()),
        fact("Kind", report_kind(report.kind)),
        fact("Trigger", report_trigger(report.trigger)),
        fact("Created", &report.created_at),
        fact("Operation", &report.operation),
        fact("Profile", report.profile.as_deref().unwrap_or("default")),
        fact("Simile", &report.simile_version),
        fact("Service version", &report.service_version),
        fact("Contactable", if report.contactable { "yes" } else { "no" }),
        Line::raw(""),
        Line::styled("DIAGNOSTIC", Style::default().fg(MUTED)),
        fact("Code", &report.diagnostic.code),
        Line::styled(&report.diagnostic.message, Style::default().fg(TEXT)),
        Line::raw(""),
        Line::styled("VERIFICATION", Style::default().fg(MUTED)),
        fact("Checked", &report.verification.checked_at),
        fact(
            "Reproduced",
            if report.verification.reproduced {
                "yes"
            } else {
                "no"
            },
        ),
        fact("Fingerprint", &report.verification.fingerprint),
        fact("Artifact", &report.artifact_digest),
    ];
    if let Some(description) = &report.description {
        lines.insert(2, Line::styled(description, Style::default().fg(MUTED)));
    }
    if !report.upgrade_recommendations.is_empty() {
        lines.extend([
            Line::raw(""),
            Line::styled("RECOMMENDED UPGRADES", Style::default().fg(MUTED)),
        ]);
        lines.extend(report.upgrade_recommendations.iter().map(|upgrade| {
            Line::from(vec![
                Span::styled(&upgrade.component, Style::default().fg(AMBER)),
                Span::raw(format!(
                    "  {} → {}  {}",
                    upgrade.current, upgrade.available, upgrade.command
                )),
            ])
        }));
    }
    Text::from(lines)
}

const fn report_kind(kind: ReportKind) -> &'static str {
    match kind {
        ReportKind::InvariantViolation => "invariant violation",
        ReportKind::EmulationInconsistency => "emulation inconsistency",
    }
}

const fn report_trigger(trigger: ReportTrigger) -> &'static str {
    match trigger {
        ReportTrigger::Automatic => "automatic",
        ReportTrigger::User => "user",
    }
}

fn report_status_style(report: &ReportDocument) -> Style {
    Style::default().fg(if report.verification.reproduced {
        GREEN
    } else {
        AMBER
    })
}

fn draw_fidelity(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let panes = master_detail(area, app.focus, app.detail_mode);
    let height = usize::from(panes[0].height.saturating_sub(4));
    let (start, end) = visible_window(service.findings.len(), app.cursor(), height);
    let rows = service.findings[start..end]
        .iter()
        .enumerate()
        .map(|(offset, finding)| {
            let index = start + offset;
            Row::new(vec![
                Cell::from(format!("{:?}", finding.severity).to_uppercase())
                    .style(severity_style(finding.severity)),
                Cell::from(finding.code.as_str()).style(Style::default().fg(CYAN)),
                Cell::from(finding.subject.as_str()),
            ])
            .style(row_style(index == app.cursor(), app.focus == Focus::List))
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(9),
                Constraint::Percentage(42),
                Constraint::Percentage(48),
            ],
        )
        .header(table_header(["SEVERITY", "CODE", "SUBJECT"]))
        .block(panel(
            &format!(
                " Findings · {} errors · {} warnings",
                service.metrics.errors, service.metrics.warnings
            ),
            app.focus == Focus::List,
        )),
        panes[0],
    );
    let finding = service
        .findings
        .get(app.cursor().min(service.findings.len().saturating_sub(1)));
    let exact = service
        .metrics
        .outcomes
        .saturating_sub(service.metrics.schema_synthesized_outcomes);
    let detail = finding.map_or_else(
        || {
            Text::from(vec![
                status_line("Public export safe", service.artifact.public_export_safe),
                Line::raw(""),
                fact("Operations executable", service.metrics.operations),
                fact(
                    "Outcomes reviewed",
                    format!("{exact}/{}", service.metrics.outcomes),
                ),
                fact(
                    "Provider authored",
                    service.metrics.provider_authored_outcomes,
                ),
                fact(
                    "Schema synthesized",
                    service.metrics.schema_synthesized_outcomes,
                ),
                fact("Underspecified", service.metrics.underspecified_operations),
            ])
        },
        |finding| {
            Text::from(vec![
                Line::styled(
                    format!("{:?}", finding.severity).to_uppercase(),
                    severity_style(finding.severity),
                ),
                Line::styled(&finding.code, Style::default().fg(CYAN)),
                Line::raw(""),
                fact("Subject", &finding.subject),
                Line::raw(""),
                Line::raw(&finding.message),
                Line::raw(""),
                Line::styled(
                    "This finding stays visible; compilation does not turn a gap into fidelity.",
                    Style::default().fg(MUTED),
                ),
            ])
        },
    );
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel(" Fidelity detail", app.focus == Focus::Detail))
            .scroll((app.detail_scroll, 0))
            .wrap(Wrap { trim: false }),
        panes[1],
    );
}

fn draw_settings(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let panes = master_detail(area, app.focus, app.detail_mode);
    let active_profile = service
        .profiles
        .iter()
        .find(|profile| profile.status == "active")
        .map_or("none", |profile| profile.name.as_str());
    let configuration = vec![
        Line::styled("REGISTERED SERVICE", Style::default().fg(MUTED)),
        Line::raw(""),
        fact("Name", &service.service.name),
        fact("Version", &service.service.version),
        fact("Listener", service.service.listen),
        fact("Active profile", active_profile),
        fact("Profiles", service.profiles.len()),
        Line::raw(""),
        Line::styled("CONFIGURATION SOURCES", Style::default().fg(MUTED)),
        Line::raw(""),
        fact("Simfile", &service.service.source),
        fact("Contract", &service.service.contract),
        fact("Contract version", &service.service.openapi_version),
        fact("Revision", &service.service.revision),
        Line::raw(""),
        Line::styled(
            if app.settings_document == Some(SettingsDocument::Simfile) {
                "▾ Simfile source · HCL"
            } else {
                "▸ Simfile source                         → inspect"
            },
            row_style(app.cursor() == 0, app.focus == Focus::List),
        ),
        Line::styled(
            if app.settings_document == Some(SettingsDocument::Contract) {
                "▾ Contract source"
            } else {
                "▸ Contract source                        → inspect"
            },
            row_style(app.cursor() == 1, app.focus == Focus::List),
        ),
        Line::raw(""),
        Line::styled(
            "Registered source snapshots stay local to this machine and are not sent to the browser console.",
            Style::default().fg(MUTED),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(configuration))
            .block(panel(" Service configuration", app.focus == Focus::List))
            .wrap(Wrap { trim: false }),
        panes[0],
    );

    let preferences = Text::from(vec![
        Line::styled("LOCAL CONSOLE", Style::default().fg(MUTED)),
        Line::raw(""),
        fact(
            "Live refresh",
            if app.auto_refresh {
                "enabled"
            } else {
                "paused"
            },
        ),
        fact(
            "Issue sounds",
            if app.sound_enabled {
                "enabled"
            } else {
                "muted"
            },
        ),
        fact(
            "Runtime",
            if app.runtime.connected() {
                "connected"
            } else {
                "offline"
            },
        ),
        Line::raw(""),
        Line::styled("SHORTCUTS", Style::default().fg(MUTED)),
        Line::raw(""),
        help_line("s", "select another registered service"),
        help_line("1–9", "jump directly to a console section"),
        help_line("o", "jump directly to operations"),
        help_line("R", "jump directly to prepared reports"),
        help_line("n", "toggle notification sounds"),
        help_line("g / G", "open logs / show logs full screen"),
        help_line("d / D", "widen detail / show detail full screen"),
        help_line("a", "pause or resume live refresh"),
        help_line("r", "refresh runtime inspection now"),
        Line::raw(""),
        Line::styled(
            "You can also click the service name in the top bar to switch services.",
            Style::default().fg(MUTED),
        ),
    ]);
    match app.settings_document {
        Some(SettingsDocument::Simfile) => draw_source_viewer(
            frame,
            panes[1],
            " Simfile · HCL",
            &service.config_contents,
            SourceSyntax::Hcl,
            app,
        ),
        Some(SettingsDocument::Contract) => draw_source_viewer(
            frame,
            panes[1],
            " Contract source",
            service.contract_contents.as_deref().unwrap_or(
                "Contract source is unavailable for this older registered revision. Re-register the service to capture an immutable snapshot.",
            ),
            SourceSyntax::Contract,
            app,
        ),
        None => frame.render_widget(
            Paragraph::new(preferences)
                .block(panel(" Preferences", app.focus == Focus::Detail))
                .scroll((app.detail_scroll, 0))
                .wrap(Wrap { trim: false }),
            panes[1],
        ),
    }
}

fn draw_source_viewer(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    source: &str,
    syntax: SourceSyntax,
    app: &App,
) {
    let panel_title = format!("{title} · j/k scroll · ← back");
    let block = panel(&panel_title, app.focus == Focus::Detail);
    let inner = block.inner(area).inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    frame.render_widget(block, area);
    let capacity = usize::from(inner.height);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(inner);
    let (numbers, source) = source_window(
        source,
        syntax,
        usize::from(columns[1].width),
        usize::from(app.detail_scroll),
        capacity,
    );
    frame.render_widget(Paragraph::new(numbers), columns[0]);
    frame.render_widget(Paragraph::new(source), columns[1]);
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let content = if let Some((notice, color)) = app.notice() {
        Line::styled(format!(" {notice}"), Style::default().fg(color))
    } else if app.query_mode {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(GREEN)),
            Span::styled(&app.query, Style::default().fg(TEXT)),
            Span::styled("█", Style::default().fg(GREEN)),
            Span::styled("  enter apply · esc close", Style::default().fg(MUTED)),
        ])
    } else {
        Line::from(vec![
            key("1–9"),
            Span::raw(" jump  "),
            key("o"),
            Span::raw(" ops  "),
            key("R"),
            Span::raw(" reports  "),
            key("g"),
            Span::raw(" logs  "),
            key("G"),
            Span::raw(" full  "),
            key("d/D"),
            Span::raw(" detail  "),
            key("tab"),
            Span::raw(" focus  "),
            key("j/k"),
            Span::raw(" move  "),
            key("←/→"),
            Span::raw(" index/detail  "),
            key("?"),
            Span::raw(" help"),
        ])
    };
    frame.render_widget(
        Paragraph::new(content)
            .style(Style::default().fg(MUTED))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(BORDER)),
            ),
        area,
    );
}

fn draw_service_switcher(frame: &mut Frame<'_>, session: &ConsoleSession, app: &App) {
    let height = (session.catalog().services.len() as u16)
        .saturating_mul(2)
        .saturating_add(4)
        .min(frame.area().height.saturating_sub(4));
    let area = centered_rect(58, height, frame.area());
    frame.render_widget(Clear, area);
    let lines = session
        .catalog()
        .services
        .iter()
        .enumerate()
        .flat_map(|(index, service)| {
            let active = session.selected().service.name == service.name;
            let cursor = app.service_cursor == index;
            [
                Line::styled(
                    format!(" {} {}", if active { "●" } else { " " }, service.name),
                    row_style(cursor, true),
                ),
                Line::styled(
                    format!("   {} · {}", service.version, service.listen),
                    Style::default()
                        .fg(MUTED)
                        .bg(if cursor { GREEN_MUTED } else { BACKGROUND }),
                ),
            ]
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).block(panel(
            " Switch local service · enter select · esc close",
            true,
        )),
        area,
    );
}

fn draw_confirmation(frame: &mut Frame<'_>, confirmation: Confirmation) {
    let area = centered_rect(62, 10, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::styled(confirmation.title(), Style::default().fg(RED)),
            Line::raw(""),
            Line::raw(confirmation.detail()),
            Line::styled("This cannot be undone.", Style::default().fg(MUTED)),
            Line::raw(""),
            Line::from(vec![
                Span::styled(" y / enter ", Style::default().fg(BACKGROUND).bg(RED)),
                Span::raw(" confirm     "),
                Span::styled(" n / esc ", Style::default().fg(TEXT).bg(BORDER_STRONG)),
                Span::raw(" cancel"),
            ]),
        ]))
        .block(panel(" Confirmation", true))
        .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_help(frame: &mut Frame<'_>) {
    let area = centered_rect(68, 25, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            help_line(
                "tab / shift-tab",
                "move between navigation, list, and detail",
            ),
            help_line(
                "j/k or arrows",
                "move selection or scroll the focused detail",
            ),
            help_line("enter", "inspect or expand the selected item in context"),
            help_line("← / →", "collapse or expand nested details"),
            help_line("1–9", "jump directly to the numbered sidebar section"),
            help_line("o", "jump directly to operations"),
            help_line("R", "jump directly to prepared reports"),
            help_line("/", "filter operations, exchanges, state, or work"),
            help_line("s", "switch registered local service"),
            help_line("n", "toggle sound for new issues and disconnects"),
            help_line("g", "open or close the live log dock"),
            help_line("G", "show logs full screen or restore the split view"),
            help_line("d", "widen or restore the focused detail pane"),
            help_line("D", "show detail full screen or restore the wide view"),
            help_line("r", "refresh all live runtime views now"),
            help_line("a", "pause or resume one-second live refresh"),
            help_line("x", "clear exchanges or reset state with confirmation"),
            help_line("p", "verify a report for the selected exchange"),
            help_line("mouse wheel", "move selection or scroll details"),
            help_line("esc", "clear search or return to navigation"),
            help_line("q / ctrl-c", "close the console"),
            Line::raw(""),
            Line::styled("Press ? or esc to close", Style::default().fg(MUTED)),
        ]))
        .block(panel(" Keyboard and mouse", true)),
        area,
    );
}

fn metric_card(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    value: impl ToString,
    positive: bool,
) {
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::styled(label, Style::default().fg(MUTED)),
            Line::styled(
                value.to_string(),
                Style::default().fg(if positive { TEXT } else { AMBER }),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER)),
        ),
        area,
    );
}

fn panel(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style(focused))
        .style(Style::default().bg(BACKGROUND).fg(TEXT))
}

fn border_style(focused: bool) -> Style {
    Style::default().fg(if focused { GREEN } else { BORDER })
}

fn master_detail(area: Rect, focus: Focus, mode: DetailMode) -> std::rc::Rc<[Rect]> {
    if mode == DetailMode::Full && focus == Focus::Detail {
        return vec![Rect::default(), area].into();
    }
    if area.width < 90 {
        // Why: a terminal cell cannot preserve a useful master/detail split at narrow widths.
        // Keep the same interaction by letting Enter/Tab swap between full-width panes.
        return if focus == Focus::Detail {
            vec![Rect::default(), area].into()
        } else {
            vec![area, Rect::default()].into()
        };
    }
    let constraints = if mode == DetailMode::Expanded {
        [Constraint::Percentage(32), Constraint::Percentage(68)]
    } else {
        [Constraint::Percentage(52), Constraint::Percentage(48)]
    };
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area)
}

fn table_header<const N: usize>(labels: [&'static str; N]) -> Row<'static> {
    Row::new(labels)
        .style(Style::default().fg(MUTED))
        .bottom_margin(1)
}

fn row_style(selected: bool, focused: bool) -> Style {
    if selected {
        let style = Style::default().fg(TEXT).bg(GREEN_MUTED);
        if focused {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    } else {
        Style::default().fg(TEXT)
    }
}

fn fact(label: impl ToString, value: impl ToString) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:<20}", label.to_string()),
            Style::default().fg(MUTED),
        ),
        Span::styled(value.to_string(), Style::default().fg(TEXT)),
    ])
}

fn status_line(label: &str, healthy: bool) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            if healthy { "● " } else { "○ " },
            Style::default().fg(if healthy { GREEN } else { RED }),
        ),
        Span::styled(format!("{label:<18}"), Style::default().fg(MUTED)),
        Span::styled(
            if healthy { "ready" } else { "unavailable" },
            Style::default().fg(TEXT),
        ),
    ])
}

fn key(value: &'static str) -> Span<'static> {
    Span::styled(value, Style::default().fg(GREEN))
}

fn help_line<'a>(keys: &'a str, description: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{keys:<18}"), Style::default().fg(GREEN)),
        Span::raw(description),
    ])
}

fn method_style(method: &str) -> Style {
    Style::default().fg(match method {
        "GET" => CYAN,
        "POST" => GREEN,
        "PUT" | "PATCH" => AMBER,
        "DELETE" => RED,
        _ => MUTED,
    })
}

fn status_style(status: u16) -> Style {
    Style::default().fg(if status < 400 {
        GREEN
    } else if status < 500 {
        AMBER
    } else {
        RED
    })
}

fn severity_style(severity: ops::AuditSeverity) -> Style {
    Style::default().fg(match severity {
        ops::AuditSeverity::Info => BLUE,
        ops::AuditSeverity::Warning => AMBER,
        ops::AuditSeverity::Error => RED,
    })
}

fn provenance_label(provenance: &str) -> &'static str {
    match provenance {
        "open_api_derived" => "OpenAPI example",
        "provider_authored" => "Provider authored",
        "schema_synthesized" => "Schema synthesized",
        _ => "Compiled outcome",
    }
}

fn selected<'a, T>(items: &'a [&'a T], cursor: usize) -> Option<&'a T> {
    items
        .get(cursor.min(items.len().saturating_sub(1)))
        .copied()
}

fn visible_window(len: usize, cursor: usize, capacity: usize) -> (usize, usize) {
    if len == 0 || capacity == 0 {
        return (0, 0);
    }
    let cursor = cursor.min(len - 1);
    let start = cursor.saturating_sub(capacity.saturating_sub(1));
    (start, (start + capacity).min(len))
}

fn filtered_operations<'a>(
    service: &'a ops::ConsoleService,
    query: &str,
) -> Vec<&'a ops::ConsoleOperation> {
    service
        .operations
        .iter()
        .filter(|operation| {
            matches_query(
                query,
                &[
                    &operation.id,
                    &operation.method,
                    &operation.path,
                    &operation.summary,
                ],
            )
        })
        .collect()
}

fn filtered_exchanges<'a>(exchanges: &'a [Exchange], query: &str) -> Vec<&'a Exchange> {
    let query = query.trim().to_ascii_lowercase();
    exchanges
        .iter()
        .filter(|exchange| {
            query.is_empty()
                || exchange.id.to_ascii_lowercase().contains(&query)
                || exchange.operation.to_ascii_lowercase().contains(&query)
                || exchange.method.to_ascii_lowercase().contains(&query)
                || exchange.path.to_ascii_lowercase().contains(&query)
                || exchange.status.to_string().contains(&query)
        })
        .collect()
}

fn runtime_logs(runtime: &RuntimeViews) -> Vec<LogEntry> {
    let mut entries = runtime
        .exchanges
        .exchanges
        .iter()
        .map(|exchange| LogEntry {
            timestamp_ms: i64::try_from(exchange.started_at_ms).unwrap_or(i64::MAX),
            level: if exchange.status >= 500 {
                "ERROR"
            } else if exchange.status >= 400 {
                "WARN"
            } else {
                "INFO"
            },
            source: "HTTP",
            summary: format!(
                "{} {} → {} · {}",
                exchange.method,
                exchange.path,
                exchange.status,
                format_duration(exchange.duration_micros)
            ),
            context: [
                Some(exchange.operation.as_str()),
                exchange.outcome.as_deref(),
                exchange.profile.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
        })
        .collect::<Vec<_>>();
    entries.extend(runtime.state.events.iter().filter_map(|event| {
        let resource = event
            .pointer("/resource/name")
            .and_then(Value::as_str)
            .unwrap_or("resource");
        // Why: console logs describe the emulated service. Simile's own profile and delivery
        // bookkeeping is implementation noise and would drown out the user's API activity.
        if resource.starts_with("__simile_") {
            return None;
        }
        let timestamp_ms = event.get("at").and_then(Value::as_i64).unwrap_or_default();
        let status = event
            .pointer("/outcome/status")
            .and_then(Value::as_str)
            .unwrap_or("succeeded");
        let kind = event
            .pointer("/resource/kind")
            .and_then(Value::as_str)
            .unwrap_or("stack");
        let action = event
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("activity");
        let sequence = event
            .get("sequence")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let operation = event
            .get("operationId")
            .and_then(Value::as_str)
            .map_or_else(String::new, |operation| format!(" · {operation}"));
        Some(LogEntry {
            timestamp_ms,
            level: if status == "failed" { "ERROR" } else { "INFO" },
            source: "STACK",
            summary: format!("{kind}.{resource} · {action} · {status}"),
            context: format!("event #{sequence}{operation}"),
        })
    }));
    entries.sort_by(|left, right| right.timestamp_ms.cmp(&left.timestamp_ms));
    entries
}

fn filtered_resources<'a>(resources: &'a [ResourceView], query: &str) -> Vec<&'a ResourceView> {
    resources
        .iter()
        .filter(|resource| matches_query(query, &[&resource.address.kind, &resource.address.name]))
        .collect()
}

fn filtered_deferred<'a>(items: &'a [DeferredItem], query: &str) -> Vec<&'a DeferredItem> {
    items
        .iter()
        .filter(|item| {
            matches_query(
                query,
                &[
                    &item.id,
                    &item.queue,
                    value_string(&item.work, "name").unwrap_or_default(),
                    value_string(&item.work, "operation").unwrap_or_default(),
                ],
            )
        })
        .collect()
}

fn matches_query(query: &str, values: &[&str]) -> bool {
    let query = query.trim().to_ascii_lowercase();
    query.is_empty()
        || values
            .iter()
            .any(|value| value.to_ascii_lowercase().contains(&query))
}

fn searchable_title(label: &str, count: usize, app: &App) -> String {
    if app.query.is_empty() {
        format!("{label} · {count}")
    } else {
        format!("{label} · {count} matching /{}", app.query)
    }
}

fn value_string<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn deferred_status(item: &DeferredItem) -> &'static str {
    if item.queue == "dead_letter" {
        "FAILED"
    } else if item.leased {
        "LEASED"
    } else {
        "PENDING"
    }
}

fn deferred_color(item: &DeferredItem) -> Color {
    if item.queue == "dead_letter" {
        RED
    } else if item.leased {
        GREEN
    } else {
        MUTED
    }
}

fn message_line(message: &Message) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            message
                .content_type
                .clone()
                .unwrap_or_else(|| "no content type".to_owned()),
            Style::default().fg(CYAN),
        ),
        Span::styled(
            if message.truncated {
                " · body omitted"
            } else {
                " · sanitized capture"
            },
            Style::default().fg(if message.truncated { AMBER } else { MUTED }),
        ),
    ])
}

fn message_body_lines(message: &Message, limit: usize) -> Vec<Line<'static>> {
    if message.truncated {
        return vec![Line::styled(
            "Body omitted because it exceeds the inspection limit.",
            Style::default().fg(AMBER),
        )];
    }
    message.body.as_ref().map_or_else(
        || vec![Line::styled("No body", Style::default().fg(MUTED))],
        |body| json_lines(body, limit),
    )
}

fn json_lines(value: &Value, limit: usize) -> Vec<Line<'static>> {
    let rendered = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_owned());
    let mut lines = rendered
        .lines()
        .take(limit)
        .map(|line| Line::styled(line.to_owned(), json_style(line)))
        .collect::<Vec<_>>();
    if rendered.lines().count() > limit {
        lines.push(Line::styled("…", Style::default().fg(MUTED)));
    }
    lines
}

fn source_window(
    source: &str,
    syntax: SourceSyntax,
    width: usize,
    start: usize,
    capacity: usize,
) -> (Text<'static>, Text<'static>) {
    if capacity == 0 {
        return (Text::default(), Text::default());
    }
    let mut numbers = Vec::with_capacity(capacity);
    let mut content = Vec::with_capacity(capacity);
    let mut visual_index = 0;
    let mut block_scalar_indent = None;
    'source: for (source_index, line) in source.lines().enumerate() {
        let leading = line
            .chars()
            .take_while(|value| value.is_whitespace())
            .count();
        // Why: YAML block scalars are prose, so wrapped rows must keep the source line's
        // left edge. A code-style hanging indent makes one paragraph look nested.
        let prose = block_scalar_indent.is_some_and(|indent| {
            line.trim().is_empty() || (!line.trim().is_empty() && leading > indent)
        });
        if block_scalar_indent.is_some() && !prose && !line.trim().is_empty() {
            block_scalar_indent = None;
        }
        for (part_index, spans) in wrap_source_line(line, syntax, width, prose)
            .into_iter()
            .enumerate()
        {
            if visual_index >= start {
                numbers.push(if part_index == 0 {
                    Line::styled(
                        format!("{:>4}  ", source_index + 1),
                        Style::default().fg(BORDER_STRONG),
                    )
                } else {
                    Line::styled("   ·  ", Style::default().fg(BORDER_STRONG))
                });
                content.push(Line::from(spans));
                if content.len() == capacity {
                    break 'source;
                }
            }
            visual_index += 1;
        }
        if syntax == SourceSyntax::Contract && is_block_scalar_header(line) {
            block_scalar_indent = Some(leading);
        }
    }
    (Text::from(numbers), Text::from(content))
}

fn wrap_source_line(
    line: &str,
    syntax: SourceSyntax,
    width: usize,
    prose: bool,
) -> Vec<Vec<Span<'static>>> {
    if line.is_empty() || width == 0 {
        return vec![Vec::new()];
    }
    let styled = match syntax {
        SourceSyntax::Hcl => hcl_spans(line),
        SourceSyntax::Contract => contract_spans(line),
    };
    let characters = line.chars().collect::<Vec<_>>();
    let leading = characters
        .iter()
        .take_while(|character| character.is_whitespace())
        .count();
    let trimmed = line.trim_start();
    let hanging = if prose || trimmed.starts_with('#') || trimmed.starts_with("//") {
        leading
    } else if syntax == SourceSyntax::Contract && trimmed.starts_with("- {") {
        leading.saturating_add(4)
    } else {
        leading.saturating_add(2)
    }
    .min(width.saturating_sub(1));
    let mut wrapped = Vec::new();
    let mut start = 0;
    let mut first = true;
    while start < characters.len() {
        while !first
            && characters
                .get(start)
                .is_some_and(|value| value.is_whitespace())
        {
            start += 1;
        }
        if start >= characters.len() {
            break;
        }
        let prefix = if first { 0 } else { hanging };
        let available = width.saturating_sub(prefix).max(1);
        let limit = start.saturating_add(available).min(characters.len());
        let mut end = limit;
        if limit < characters.len() {
            let whitespace = characters[start..limit]
                .iter()
                .rposition(|character| character.is_whitespace())
                .filter(|offset| *offset > 0);
            let delimiter = characters[start..limit]
                .iter()
                .rposition(|character| matches!(character, '/' | '-' | '_' | '.' | '?' | '&'))
                .filter(|offset| *offset > 0)
                .map(|offset| offset + 1);
            if let Some(offset) = whitespace.or(delimiter) {
                end = start + offset;
            }
        }
        let mut content_end = end;
        while content_end > start && characters[content_end - 1].is_whitespace() {
            content_end -= 1;
        }
        let mut spans = if prefix == 0 {
            Vec::new()
        } else {
            vec![Span::raw(" ".repeat(prefix))]
        };
        spans.extend(slice_spans(&styled, start, content_end));
        wrapped.push(spans);
        start = end.max(start + 1);
        first = false;
    }
    if wrapped.is_empty() {
        wrapped.push(Vec::new());
    }
    wrapped
}

fn is_block_scalar_header(line: &str) -> bool {
    let trimmed = line.trim_end();
    trimmed.contains(':')
        && trimmed
            .chars()
            .next_back()
            .is_some_and(|value| matches!(value, '|' | '>'))
}

fn slice_spans(spans: &[Span<'static>], start: usize, end: usize) -> Vec<Span<'static>> {
    let mut offset = 0;
    let mut sliced = Vec::new();
    for span in spans {
        let characters = span.content.chars().collect::<Vec<_>>();
        let span_end = offset + characters.len();
        let overlap_start = start.max(offset);
        let overlap_end = end.min(span_end);
        if overlap_start < overlap_end {
            sliced.push(Span::styled(
                characters[overlap_start - offset..overlap_end - offset]
                    .iter()
                    .collect::<String>(),
                span.style,
            ));
        }
        offset = span_end;
        if offset >= end {
            break;
        }
    }
    sliced
}

fn contract_spans(line: &str) -> Vec<Span<'static>> {
    // Why: highlighting the whole line as one key/value pair breaks as soon as a flow
    // object wraps. Token styles must be assigned before spans are split into visual rows.
    let characters = line.chars().collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut cursor = 0;
    while cursor < characters.len() {
        if characters[cursor].is_whitespace() {
            let start = cursor;
            while cursor < characters.len() && characters[cursor].is_whitespace() {
                cursor += 1;
            }
            spans.push(Span::raw(
                characters[start..cursor].iter().collect::<String>(),
            ));
            continue;
        }
        if characters[cursor] == '#'
            || (characters[cursor] == '/'
                && characters.get(cursor + 1).is_some_and(|next| *next == '/'))
        {
            spans.push(Span::styled(
                characters[cursor..].iter().collect::<String>(),
                Style::default().fg(MUTED),
            ));
            break;
        }
        if matches!(characters[cursor], '"' | '\'') {
            let quote = characters[cursor];
            let start = cursor;
            cursor += 1;
            let mut escaped = false;
            while cursor < characters.len() {
                let character = characters[cursor];
                cursor += 1;
                if character == quote && !escaped {
                    break;
                }
                escaped = character == '\\' && !escaped;
                if character != '\\' {
                    escaped = false;
                }
            }
            spans.push(Span::styled(
                characters[start..cursor].iter().collect::<String>(),
                Style::default().fg(if next_nonspace_is_colon(&characters, cursor) {
                    CYAN
                } else {
                    GREEN
                }),
            ));
            continue;
        }
        if matches!(
            characters[cursor],
            ':' | '{' | '}' | '[' | ']' | ',' | '|' | '>'
        ) {
            spans.push(Span::styled(
                characters[cursor].to_string(),
                Style::default().fg(BORDER_STRONG),
            ));
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 1;
        while cursor < characters.len()
            && !characters[cursor].is_whitespace()
            && !matches!(
                characters[cursor],
                ':' | '{' | '}' | '[' | ']' | ',' | '#' | '"' | '\''
            )
        {
            cursor += 1;
        }
        let token = characters[start..cursor].iter().collect::<String>();
        let color = if next_nonspace_is_colon(&characters, cursor) || token == "-" {
            CYAN
        } else if matches!(token.as_str(), "true" | "false" | "null")
            || token.parse::<f64>().is_ok()
        {
            AMBER
        } else {
            TEXT
        };
        spans.push(Span::styled(token, Style::default().fg(color)));
    }
    spans
}

fn next_nonspace_is_colon(characters: &[char], cursor: usize) -> bool {
    characters[cursor..]
        .iter()
        .find(|character| !character.is_whitespace())
        .is_some_and(|character| *character == ':')
}

fn hcl_spans(line: &str) -> Vec<Span<'static>> {
    let chars = line.chars().collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut cursor = 0;
    while cursor < chars.len() {
        if chars[cursor].is_whitespace() {
            let start = cursor;
            while cursor < chars.len() && chars[cursor].is_whitespace() {
                cursor += 1;
            }
            spans.push(Span::raw(chars[start..cursor].iter().collect::<String>()));
            continue;
        }
        if chars[cursor] == '#'
            || (chars[cursor] == '/' && chars.get(cursor + 1).is_some_and(|next| *next == '/'))
        {
            spans.push(Span::styled(
                chars[cursor..].iter().collect::<String>(),
                Style::default().fg(MUTED),
            ));
            break;
        }
        if chars[cursor] == '"' {
            let start = cursor;
            cursor += 1;
            let mut escaped = false;
            while cursor < chars.len() {
                let character = chars[cursor];
                cursor += 1;
                if character == '"' && !escaped {
                    break;
                }
                escaped = character == '\\' && !escaped;
                if character != '\\' {
                    escaped = false;
                }
            }
            spans.push(Span::styled(
                chars[start..cursor].iter().collect::<String>(),
                Style::default().fg(GREEN),
            ));
            continue;
        }
        if chars[cursor].is_ascii_digit()
            || (chars[cursor] == '-'
                && chars
                    .get(cursor + 1)
                    .is_some_and(|next| next.is_ascii_digit()))
        {
            let start = cursor;
            cursor += 1;
            while cursor < chars.len()
                && (chars[cursor].is_ascii_digit() || matches!(chars[cursor], '.' | '_'))
            {
                cursor += 1;
            }
            spans.push(Span::styled(
                chars[start..cursor].iter().collect::<String>(),
                Style::default().fg(AMBER),
            ));
            continue;
        }
        if chars[cursor].is_alphabetic() || chars[cursor] == '_' {
            let start = cursor;
            cursor += 1;
            while cursor < chars.len()
                && (chars[cursor].is_alphanumeric() || matches!(chars[cursor], '_' | '-'))
            {
                cursor += 1;
            }
            let word = chars[start..cursor].iter().collect::<String>();
            let next = chars[cursor..]
                .iter()
                .find(|character| !character.is_whitespace())
                .copied();
            let color = if matches!(word.as_str(), "true" | "false" | "null") {
                AMBER
            } else if next == Some('=') {
                CYAN
            } else if next == Some('{') || is_hcl_keyword(&word) {
                BLUE
            } else {
                TEXT
            };
            spans.push(Span::styled(word, Style::default().fg(color)));
            continue;
        }
        spans.push(Span::styled(
            chars[cursor].to_string(),
            Style::default().fg(BORDER_STRONG),
        ));
        cursor += 1;
    }
    spans
}

fn is_hcl_keyword(word: &str) -> bool {
    matches!(
        word,
        "service"
            | "listen"
            | "openapi"
            | "smithy"
            | "typescript"
            | "behavior"
            | "behaviors"
            | "scenario"
            | "outcome"
            | "fault"
            | "profile"
            | "principals"
            | "principal_setup"
            | "resource"
            | "dependency"
    )
}

fn json_style(line: &str) -> Style {
    if line.trim_start().starts_with('"') {
        Style::default().fg(CYAN)
    } else if line.contains("true") || line.contains("false") || line.contains("null") {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(TEXT)
    }
}

fn format_duration(micros: u64) -> String {
    if micros < 1_000 {
        format!("{micros}µs")
    } else if micros < 1_000_000 {
        format!("{:.1}ms", micros as f64 / 1_000.0)
    } else {
        format!("{:.2}s", micros as f64 / 1_000_000.0)
    }
}

fn format_clock(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!(
        "{:02}:{:02}:{:02}",
        (seconds / 3_600) % 24,
        (seconds / 60) % 60,
        seconds % 60
    )
}

fn format_log_timestamp(milliseconds: i64) -> String {
    let nanos = i128::from(milliseconds).saturating_mul(1_000_000);
    let Ok(utc) = OffsetDateTime::from_unix_timestamp_nanos(nanos) else {
        return "0000-00-00 00:00:00".to_owned();
    };
    let local = UtcOffset::current_local_offset().map_or(utc, |offset| utc.to_offset(offset));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.year(),
        u8::from(local.month()),
        local.day(),
        local.hour(),
        local.minute(),
        local.second()
    )
}

fn short_digest(digest: &str) -> String {
    if digest.len() <= 16 {
        return digest.to_owned();
    }
    format!("sha256:{}…{}", &digest[..8], &digest[digest.len() - 4..])
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AMBER, BLUE, CYAN, DetailMode, ExchangeHistory, Focus, GREEN, LogDock, MUTED, Page,
        RuntimeViews, SourceSyntax, StateView, format_duration, hcl_spans, log_dock_height,
        log_window, master_detail, runtime_logs, source_window, viewport_scroll, visible_window,
    };

    #[test]
    fn runtime_views_decode_live_console_payloads() {
        let exchanges = serde_json::from_value::<ExchangeHistory>(serde_json::json!({
            "exchanges": [{
                "id": "ex_1",
                "operation": "POST /orders",
                "method": "POST",
                "path": "/orders",
                "startedAtMs": 1,
                "durationMicros": 1200,
                "status": 201,
                "request": { "body": {}, "truncated": false },
                "response": { "body": {}, "truncated": false }
            }]
        }))
        .unwrap_or_else(|error| panic!("{error}"));
        let state = serde_json::from_value::<StateView>(serde_json::json!({
            "resources": [{ "address": { "kind": "db", "name": "orders" }, "value": {} }],
            "events": []
        }))
        .unwrap_or_else(|error| panic!("{error}"));
        let runtime = RuntimeViews {
            exchanges,
            state,
            ..RuntimeViews::default()
        };
        assert_eq!(runtime.exchanges.exchanges[0].status, 201);
        assert_eq!(runtime.state.resources[0].address.name, "orders");
    }

    #[test]
    fn visible_window_keeps_selection_on_screen() {
        assert_eq!(visible_window(100, 0, 10), (0, 10));
        assert_eq!(visible_window(100, 12, 10), (3, 13));
        assert_eq!(visible_window(4, 3, 10), (0, 4));
    }

    #[test]
    fn detail_scroll_starts_only_after_selection_crosses_the_viewport() {
        assert_eq!(viewport_scroll(0, 8, 40, 30), 0);
        assert_eq!(viewport_scroll(0, 29, 40, 30), 0);
        assert_eq!(viewport_scroll(0, 30, 40, 30), 1);
        assert_eq!(viewport_scroll(0, 31, 40, 30), 2);
        assert_eq!(viewport_scroll(12, 8, 20, 30), 0);
    }

    #[test]
    fn detail_modes_preserve_an_index_widening_step_before_full_screen() {
        assert_eq!(DetailMode::Split.toggle(), DetailMode::Expanded);
        assert_eq!(DetailMode::Expanded.toggle(), DetailMode::Split);
        assert_eq!(DetailMode::Expanded.toggle_full(), DetailMode::Full);
        assert_eq!(DetailMode::Full.toggle_full(), DetailMode::Expanded);

        let area = ratatui::layout::Rect::new(0, 0, 120, 30);
        let split = master_detail(area, Focus::Detail, DetailMode::Split);
        let expanded = master_detail(area, Focus::Detail, DetailMode::Expanded);
        let full = master_detail(area, Focus::Detail, DetailMode::Full);
        assert!(expanded[1].width > split[1].width);
        assert_eq!(full[0], ratatui::layout::Rect::default());
        assert_eq!(full[1], area);
    }

    #[test]
    fn durations_are_compact_and_terminal_friendly() {
        assert_eq!(format_duration(999), "999µs");
        assert_eq!(format_duration(1_200), "1.2ms");
        assert_eq!(format_duration(2_000_000), "2.00s");
    }

    #[test]
    fn sidebar_shortcuts_are_stable_and_hcl_is_semantically_highlighted() {
        assert_eq!(
            Page::ALL.map(Page::shortcut),
            ["1", "2", "3", "4", "5", "6", "7", "R", "8", "9"]
        );

        let source = "service \"orders\" { # local configuration";
        let spans = hcl_spans(source);
        assert_eq!(
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            source
        );
        assert_eq!(spans[0].style.fg, Some(BLUE));
        assert!(spans.iter().any(|span| span.style.fg == Some(GREEN)));
        assert_eq!(spans.last().and_then(|span| span.style.fg), Some(MUTED));
    }

    #[test]
    fn source_viewer_wraps_with_hanging_gutters_and_preserved_highlighting() {
        let source = format!(
            "service \"orders\" {{\n  description = \"{}\"\n}}",
            "x".repeat(240)
        );
        let (numbers, text) = source_window(&source, SourceSyntax::Hcl, 32, 0, 20);
        assert_eq!(numbers.lines.len(), text.lines.len());
        assert!(text.lines.len() > 4);
        assert!(text.lines.iter().all(|line| line.width() <= 32));
        assert!(numbers.lines.iter().any(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains('·')
        }));
        assert!(
            text.lines
                .iter()
                .skip(2)
                .any(|line| { line.spans.iter().any(|span| span.style.fg == Some(GREEN)) })
        );
    }

    #[test]
    fn contract_prose_wraps_on_one_stable_text_edge() {
        let source = "description: |\n    Generate a one-time password and deliver it by SMS or email. Store the returned transaction ID\n    and use it at the verification endpoint.";
        let (numbers, text) = source_window(source, SourceSyntax::Contract, 44, 0, 20);
        assert_eq!(numbers.lines.len(), text.lines.len());
        let prose = text
            .lines
            .iter()
            .skip(1)
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(prose.len() > 2);
        assert!(prose.iter().all(|line| line.starts_with("    ")));
        assert!(prose.iter().all(|line| !line.starts_with("      ")));
    }

    #[test]
    fn wrapped_yaml_flow_values_keep_key_and_boolean_highlighting() {
        let source = "      - { request: /async_delivery, equals: true, default: false }";
        let (_, text) = source_window(source, SourceSyntax::Contract, 42, 0, 10);
        assert!(text.lines.len() > 1);
        assert!(text.lines.iter().all(|line| line.width() <= 42));
        assert!(
            text.lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| { span.content.as_ref() == "default" && span.style.fg == Some(CYAN) })
        );
        assert!(
            text.lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| { span.content.as_ref() == "false" && span.style.fg == Some(AMBER) })
        );
    }

    #[test]
    fn log_dock_keeps_the_newest_activity_visible_in_every_mode() {
        assert_eq!(log_dock_height(40, LogDock::Collapsed), 3);
        assert_eq!(log_dock_height(40, LogDock::Partial), 14);
        assert_eq!(log_dock_height(40, LogDock::Full), 35);
        assert_eq!(log_window(4, 1, 0), (0, 1));
        assert_eq!(log_window(4, 2, 1), (1, 3));
    }

    #[test]
    fn live_logs_merge_sanitized_http_and_payload_free_stack_activity() {
        let exchanges = serde_json::from_value::<ExchangeHistory>(serde_json::json!({
            "exchanges": [{
                "id": "ex_1",
                "operation": "createOrder",
                "method": "POST",
                "path": "/orders",
                "startedAtMs": 1000,
                "durationMicros": 1200,
                "status": 422,
                "request": { "truncated": false },
                "response": { "truncated": false }
            }]
        }))
        .unwrap_or_else(|error| panic!("{error}"));
        let state = serde_json::from_value::<StateView>(serde_json::json!({
            "resources": [],
            "events": [{
                "sequence": 1,
                "at": 2000,
                "resource": { "kind": "db", "name": "orders" },
                "action": "put",
                "outcome": { "status": "succeeded" }
            }]
        }))
        .unwrap_or_else(|error| panic!("{error}"));
        let logs = runtime_logs(&RuntimeViews {
            exchanges,
            state,
            ..RuntimeViews::default()
        });

        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].source, "STACK");
        assert_eq!(logs[1].level, "WARN");
        assert!(logs[1].context.contains("createOrder"));
        assert!(!logs[1].context.contains("request"));
    }
}
