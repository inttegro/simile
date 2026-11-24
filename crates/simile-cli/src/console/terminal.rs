use std::io::{self, IsTerminal};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Frame;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap};
use simile_core::ops;

use super::ConsoleSession;
use crate::error::{Error, Result};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Page {
    Service,
    Operations,
    Exchanges,
    State,
    Deferred,
    Scenarios,
    Faults,
    Fidelity,
}

impl Page {
    const ALL: [Self; 8] = [
        Self::Service,
        Self::Operations,
        Self::Exchanges,
        Self::State,
        Self::Deferred,
        Self::Scenarios,
        Self::Faults,
        Self::Fidelity,
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
            Self::Fidelity => "Fidelity",
        }
    }
}

struct App {
    page: usize,
    service_cursor: usize,
    selecting_service: bool,
    scroll: u16,
    runtime: RuntimeViews,
    refreshed_at: Instant,
}

#[derive(Default)]
struct RuntimeViews {
    inspection: Option<serde_json::Value>,
    exchanges: Option<serde_json::Value>,
    state: Option<serde_json::Value>,
    deferred: Option<serde_json::Value>,
}

pub(crate) fn run(runtime: &tokio::runtime::Runtime, session: &mut ConsoleSession) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::ConsoleTerminalRequired);
    }
    enable_raw_mode().map_err(Error::ConsoleRender)?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(Error::ConsoleRender)?;
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
        page: 0,
        service_cursor,
        selecting_service: false,
        scroll: 0,
        runtime: RuntimeViews::default(),
        refreshed_at: Instant::now() - REFRESH_INTERVAL,
    };

    loop {
        if app.refreshed_at.elapsed() >= REFRESH_INTERVAL {
            app.runtime = runtime.block_on(refresh_runtime(session));
            app.refreshed_at = Instant::now();
        }
        terminal
            .draw(|frame| draw(frame, session, &app))
            .map_err(Error::ConsoleRender)?;

        if !event::poll(Duration::from_millis(120)).map_err(Error::ConsoleEvent)? {
            continue;
        }
        let Event::Key(key) = event::read().map_err(Error::ConsoleEvent)? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') => break,
            KeyCode::Char('s') => app.selecting_service = !app.selecting_service,
            KeyCode::Char('r') => {
                app.runtime = runtime.block_on(refresh_runtime(session));
                app.refreshed_at = Instant::now();
            }
            KeyCode::Tab => {
                app.page = (app.page + 1) % Page::ALL.len();
                app.scroll = 0;
            }
            KeyCode::BackTab => {
                app.page = app.page.checked_sub(1).unwrap_or(Page::ALL.len() - 1);
                app.scroll = 0;
            }
            KeyCode::Right if !app.selecting_service => {
                app.page = (app.page + 1) % Page::ALL.len();
                app.scroll = 0;
            }
            KeyCode::Left if !app.selecting_service => {
                app.page = app.page.checked_sub(1).unwrap_or(Page::ALL.len() - 1);
                app.scroll = 0;
            }
            KeyCode::Down if app.selecting_service => {
                let count = session.catalog().services.len();
                app.service_cursor = (app.service_cursor + 1).min(count.saturating_sub(1));
            }
            KeyCode::Up if app.selecting_service => {
                app.service_cursor = app.service_cursor.saturating_sub(1);
            }
            KeyCode::Enter if app.selecting_service => {
                let service = session.catalog().services[app.service_cursor].name.clone();
                runtime.block_on(session.select(&service))?;
                app.selecting_service = false;
                app.scroll = 0;
                app.runtime = runtime.block_on(refresh_runtime(session));
                app.refreshed_at = Instant::now();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.scroll = app.scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.scroll = app.scroll.saturating_sub(1);
            }
            KeyCode::Home => app.scroll = 0,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            _ => {}
        }
    }
    Ok(())
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        drop(disable_raw_mode());
        let mut stdout = io::stdout();
        drop(execute!(stdout, LeaveAlternateScreen));
    }
}

async fn refresh_runtime(session: &ConsoleSession) -> RuntimeViews {
    let (inspection, exchanges, state, deferred) = tokio::join!(
        session.inspect("/.simile/inspection"),
        session.inspect("/.simile/exchanges"),
        session.inspect("/.simile/state"),
        session.inspect("/.simile/deferred"),
    );
    RuntimeViews {
        inspection,
        exchanges,
        state,
        deferred,
    }
}

fn draw(frame: &mut Frame<'_>, session: &ConsoleSession, app: &App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(frame.area());
    draw_header(frame, vertical[0], session);
    draw_tabs(frame, vertical[1], app);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(40)])
        .split(vertical[2]);
    draw_services(frame, body[0], session, app);
    draw_page(frame, body[1], session.selected(), app);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "s",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" services  "),
            Span::styled("tab/←/→", Style::default().fg(Color::Cyan)),
            Span::raw(" views  "),
            Span::styled("j/k", Style::default().fg(Color::Cyan)),
            Span::raw(" scroll  "),
            Span::styled("r", Style::default().fg(Color::Cyan)),
            Span::raw(" refresh  "),
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::raw(" quit"),
        ]))
        .style(Style::default().fg(Color::DarkGray)),
        vertical[3],
    );
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, session: &ConsoleSession) {
    let service = &session.selected().service;
    let ownership = if session.owns_runtime() {
        "started by console"
    } else {
        "attached"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "🎭 simile",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(&service.name, Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("  {}  ", service.version),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("●", Style::default().fg(Color::Green)),
            Span::raw(format!(
                " http://{} · {ownership}",
                session.runtime_address()
            )),
        ]))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::Blue)),
        ),
        area,
    );
}

fn draw_tabs(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let titles = Page::ALL
        .iter()
        .map(|page| Line::from(page.label()))
        .collect::<Vec<_>>();
    frame.render_widget(
        Tabs::new(titles)
            .select(app.page)
            .highlight_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .divider("  ")
            .padding(" ", " "),
        area,
    );
}

fn draw_services(frame: &mut Frame<'_>, area: Rect, session: &ConsoleSession, app: &App) {
    let lines = session
        .catalog()
        .services
        .iter()
        .enumerate()
        .flat_map(|(index, service)| {
            let selected = service.name == session.selected().service.name;
            let cursor = app.selecting_service && app.service_cursor == index;
            let style = if cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if selected {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            [
                Line::styled(
                    format!(" {} {}", if selected { "●" } else { " " }, service.name),
                    style,
                ),
                Line::styled(
                    format!("   {} · {}", service.version, service.listen),
                    Style::default().fg(Color::DarkGray),
                ),
            ]
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(if app.selecting_service {
                        " Services · enter to switch "
                    } else {
                        " Services · s to choose "
                    })
                    .borders(Borders::RIGHT)
                    .border_style(Style::default().fg(if app.selecting_service {
                        Color::Cyan
                    } else {
                        Color::Blue
                    })),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_page(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    match Page::ALL[app.page] {
        Page::Service => draw_service(frame, area, service, app),
        Page::Operations => draw_operations(frame, area, service, app),
        Page::Exchanges => draw_json(
            frame,
            area,
            " Exchanges ",
            app.runtime.exchanges.as_ref(),
            app.scroll,
        ),
        Page::State => draw_json(
            frame,
            area,
            " State ",
            app.runtime.state.as_ref(),
            app.scroll,
        ),
        Page::Deferred => draw_json(
            frame,
            area,
            " Deferred work ",
            app.runtime.deferred.as_ref(),
            app.scroll,
        ),
        Page::Scenarios => draw_scenarios(frame, area, service, app),
        Page::Faults => draw_faults(frame, area, service, app),
        Page::Fidelity => draw_fidelity(frame, area, service, app),
    }
}

fn draw_service(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let runtime = app.runtime.inspection.as_ref();
    let text = Text::from(vec![
        Line::styled(
            &service.service.name,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(
            service
                .service
                .description
                .as_deref()
                .unwrap_or("Local service emulator"),
        ),
        Line::raw(""),
        fact("Listener", service.service.listen),
        fact(
            "Contract",
            format!("OpenAPI {}", service.service.openapi_version),
        ),
        fact("Operations", service.metrics.operations),
        fact("Backends", service.metrics.backends),
        fact("Async operations", service.metrics.async_operations),
        fact("Scenarios", service.metrics.scenarios),
        fact("Fault profiles", service.metrics.fault_profiles),
        fact("Artifact", format!("sha256:{}", service.artifact.sha256)),
        fact(
            "Runtime",
            runtime.map_or("waiting".to_owned(), |_| "ready".to_owned()),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Service ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Blue)),
            )
            .scroll((app.scroll, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_operations(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let rows = service
        .operations
        .iter()
        .skip(usize::from(app.scroll))
        .map(|operation| {
            Row::new(vec![
                Cell::from(operation.method.clone()).style(method_style(&operation.method)),
                Cell::from(operation.path.clone()),
                Cell::from(operation.id.clone()).style(Style::default().fg(Color::Cyan)),
                Cell::from(operation.success.to_string()).style(Style::default().fg(Color::Green)),
                Cell::from(operation.outcomes.len().to_string()),
            ])
        });
    let header = Row::new(["Method", "Path", "Operation", "HTTP", "Outcomes"]).style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Percentage(32),
                Constraint::Percentage(40),
                Constraint::Length(7),
                Constraint::Length(9),
            ],
        )
        .header(header)
        .column_spacing(1)
        .block(
            Block::default()
                .title(format!(" Operations · {} ", service.operations.len()))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        ),
        area,
    );
}

fn draw_scenarios(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let lines = service
        .scenarios
        .iter()
        .skip(usize::from(app.scroll))
        .flat_map(|scenario| {
            [
                Line::from(vec![
                    Span::styled(
                        &scenario.name,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "  {} selections · {} faults",
                            scenario.selections, scenario.faults
                        ),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::raw(format!(
                    "  {}",
                    scenario.description.as_deref().unwrap_or("No description")
                )),
                Line::raw(""),
            ]
        })
        .collect::<Vec<_>>();
    draw_lines(
        frame,
        area,
        format!(" Scenarios · {} ", service.scenarios.len()),
        lines,
    );
}

fn draw_faults(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let lines = service
        .faults
        .iter()
        .skip(usize::from(app.scroll))
        .flat_map(|fault| {
            [
                Line::from(vec![
                    Span::styled(
                        &fault.name,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {} · {}", fault.phase, fault.action),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::raw(format!(
                    "  {}",
                    fault.description.as_deref().unwrap_or("No description")
                )),
                Line::raw(""),
            ]
        })
        .collect::<Vec<_>>();
    draw_lines(
        frame,
        area,
        format!(
            " Faults · {} definitions · {} profiles ",
            service.faults.len(),
            service.fault_profiles.len()
        ),
        lines,
    );
}

fn draw_fidelity(frame: &mut Frame<'_>, area: Rect, service: &ops::ConsoleService, app: &App) {
    let mut lines = vec![
        fact("Audit errors", service.metrics.errors),
        fact("Audit warnings", service.metrics.warnings),
        fact(
            "Provider-authored outcomes",
            service.metrics.provider_authored_outcomes,
        ),
        fact(
            "Schema-synthesized outcomes",
            service.metrics.schema_synthesized_outcomes,
        ),
        fact(
            "Underspecified operations",
            service.metrics.underspecified_operations,
        ),
        Line::raw(""),
    ];
    lines.extend(
        service
            .findings
            .iter()
            .skip(usize::from(app.scroll))
            .flat_map(|finding| {
                [
                    Line::from(vec![
                        Span::styled(
                            format!("{:?}", finding.severity).to_uppercase(),
                            severity_style(finding.severity),
                        ),
                        Span::raw("  "),
                        Span::styled(&finding.code, Style::default().fg(Color::Cyan)),
                        Span::raw(format!("  {}", finding.subject)),
                    ]),
                    Line::raw(format!("  {}", finding.message)),
                    Line::raw(""),
                ]
            }),
    );
    draw_lines(frame, area, " Fidelity ".to_owned(), lines);
}

fn draw_json(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    value: Option<&serde_json::Value>,
    scroll: u16,
) {
    let rendered = value.map_or_else(
        || "Waiting for the local runtime inspector…".to_owned(),
        |value| {
            serde_json::to_string_pretty(value)
                .unwrap_or_else(|_| "Runtime data is unavailable".to_owned())
        },
    );
    frame.render_widget(
        Paragraph::new(rendered)
            .style(Style::default().fg(Color::Gray))
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Blue)),
            )
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_lines(frame: &mut Frame<'_>, area: Rect, title: String, lines: Vec<Line<'_>>) {
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Blue)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn fact(label: impl ToString, value: impl ToString) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:<24}", label.to_string()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            value.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])
}

fn method_style(method: &str) -> Style {
    Style::default()
        .fg(match method {
            "GET" => Color::Cyan,
            "POST" => Color::Green,
            "PUT" | "PATCH" => Color::Yellow,
            "DELETE" => Color::Red,
            _ => Color::Gray,
        })
        .add_modifier(Modifier::BOLD)
}

fn severity_style(severity: ops::AuditSeverity) -> Style {
    Style::default()
        .fg(match severity {
            ops::AuditSeverity::Info => Color::Cyan,
            ops::AuditSeverity::Warning => Color::Yellow,
            ops::AuditSeverity::Error => Color::Red,
        })
        .add_modifier(Modifier::BOLD)
}
