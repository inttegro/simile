//! The human terminal vocabulary for Simile commands.
//!
//! Command adapters describe activities and facts through this module instead of assembling
//! ANSI sequences themselves. `Terminal` remains responsible for stdout, stderr, colour policy,
//! quiet mode and broken-pipe behavior.

use std::fmt;
use std::time::{Duration, Instant};

use anstyle::{AnsiColor, Effects, Style};

use crate::error::Result;
use crate::terminal::Terminal;

pub(crate) mod trace;

pub(crate) const BRAND: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
pub(crate) const ACTIVE: Style = AnsiColor::Cyan.on_default();
pub(crate) const SUCCESS: Style = AnsiColor::Green.on_default();
pub(crate) const WARNING: Style = AnsiColor::Yellow.on_default();
pub(crate) const FAILURE: Style = AnsiColor::Red.on_default();
pub(crate) const RAIL: Style = AnsiColor::Blue.on_default();
pub(crate) const MUTED: Style = AnsiColor::BrightBlack.on_default();
pub(crate) const EMPHASIS: Style = Style::new().effects(Effects::BOLD);

/// One row in a compact human-readable terminal table.
pub(crate) struct TableRow {
    label: String,
    value: String,
    detail: String,
}

impl TableRow {
    #[must_use]
    pub(crate) fn new(
        label: impl Into<String>,
        value: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            detail: detail.into(),
        }
    }
}

/// High-level human presentation backed by the command's terminal policy.
#[derive(Clone, Copy)]
pub(crate) struct Ui<'a> {
    terminal: &'a Terminal,
}

impl<'a> Ui<'a> {
    #[must_use]
    pub(crate) const fn new(terminal: &'a Terminal) -> Self {
        Self { terminal }
    }

    /// Introduce a finite or long-running Simile workflow.
    pub(crate) fn banner(&self) -> Result<()> {
        if !self.terminal.is_human() {
            return Ok(());
        }
        self.terminal.status_line(format_args!(
            "🎭 {BRAND}simile{BRAND:#} {MUTED}v{}{MUTED:#}",
            env!("CARGO_PKG_VERSION")
        ))
    }

    /// Start one visible unit of work and begin measuring it.
    pub(crate) fn activity(&self, message: impl fmt::Display) -> Result<Activity<'a>> {
        let visible = self.terminal.is_human();
        if visible {
            self.terminal
                .status_line(format_args!("{ACTIVE}▸{ACTIVE:#} {message}"))?;
        }
        Ok(Activity {
            ui: *self,
            started: Instant::now(),
            visible,
        })
    }

    pub(crate) fn succeeded(&self, message: impl fmt::Display, elapsed: Duration) -> Result<()> {
        if !self.terminal.is_human() {
            return Ok(());
        }
        let elapsed = format_duration(elapsed);
        self.terminal.status_line(format_args!(
            "{SUCCESS}✓{SUCCESS:#} {EMPHASIS}{message}{EMPHASIS:#} {MUTED}{elapsed}{MUTED:#}"
        ))
    }

    pub(crate) fn failed(&self, message: impl fmt::Display, elapsed: Duration) -> Result<()> {
        if !self.terminal.is_human() {
            return Ok(());
        }
        let elapsed = format_duration(elapsed);
        self.terminal.status_line(format_args!(
            "{FAILURE}✗{FAILURE:#} {EMPHASIS}{message}{EMPHASIS:#} {MUTED}{elapsed}{MUTED:#}"
        ))
    }

    pub(crate) fn note(&self, label: impl fmt::Display, value: impl fmt::Display) -> Result<()> {
        if !self.terminal.is_human() {
            return Ok(());
        }
        self.terminal.status_line(format_args!(
            "  {MUTED}{label}{MUTED:#} {EMPHASIS}{value}{EMPHASIS:#}"
        ))
    }

    /// Write the title of a completed command result to stdout.
    pub(crate) fn result_title(
        &self,
        action: impl fmt::Display,
        subject: impl fmt::Display,
    ) -> Result<()> {
        self.terminal.line(format_args!(
            "{SUCCESS}✓{SUCCESS:#} {BRAND}{action}{BRAND:#} {EMPHASIS}{subject}{EMPHASIS:#}"
        ))
    }

    /// Write one labelled result fact to stdout.
    pub(crate) fn result_fact(
        &self,
        label: impl fmt::Display,
        value: impl fmt::Display,
    ) -> Result<()> {
        self.terminal
            .line(format_args!("  {MUTED}{label}{MUTED:#} {value}"))
    }

    /// Render a small aligned table for a manifest or similarly structured result.
    pub(crate) fn table(&self, title: &str, rows: &[TableRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let (label_width, value_width) = table_widths(rows);
        self.terminal.line(format_args!(""))?;
        self.terminal
            .line(format_args!("  {EMPHASIS}{title}{EMPHASIS:#}"))?;
        self.terminal.line(format_args!(
            "  {MUTED}{}{MUTED:#}",
            "─".repeat(title.chars().count())
        ))?;
        for row in rows {
            self.terminal.line(format_args!(
                "  {ACTIVE}{:<label_width$}{ACTIVE:#} {MUTED}│{MUTED:#} {EMPHASIS}{:<value_width$}{EMPHASIS:#} {MUTED}│ {}{MUTED:#}",
                row.label, row.value, row.detail
            ))?;
        }
        self.terminal.line(format_args!(""))
    }

    /// Write one event in a long-running activity timeline.
    pub(crate) fn timeline_ready(
        &self,
        message: impl fmt::Display,
        elapsed: Duration,
    ) -> Result<()> {
        if !self.terminal.is_human() {
            return Ok(());
        }
        let elapsed = format_duration(elapsed);
        self.terminal.status_line(format_args!(
            "{RAIL}│{RAIL:#} {SUCCESS}✓{SUCCESS:#} {EMPHASIS}{message}{EMPHASIS:#} {MUTED}{elapsed}{MUTED:#}"
        ))
    }
}

/// A measured unit of CLI work.
pub(crate) struct Activity<'a> {
    ui: Ui<'a>,
    started: Instant,
    visible: bool,
}

impl Activity<'_> {
    pub(crate) fn complete(self, message: impl fmt::Display) -> Result<()> {
        if !self.visible {
            return Ok(());
        }
        self.ui.succeeded(message, self.started.elapsed())
    }

    pub(crate) fn fail(self, message: impl fmt::Display) -> Result<()> {
        if !self.visible {
            return Ok(());
        }
        self.ui.failed(message, self.started.elapsed())
    }

    pub(crate) fn complete_timeline(self, message: impl fmt::Display) -> Result<()> {
        if !self.visible {
            return Ok(());
        }
        self.ui.timeline_ready(message, self.started.elapsed())
    }
}

#[must_use]
pub(crate) fn format_duration(elapsed: Duration) -> String {
    if elapsed < Duration::from_millis(1) {
        return format!("{}µs", elapsed.as_micros());
    }
    if elapsed < Duration::from_secs(1) {
        return format!("{}ms", elapsed.as_millis());
    }
    if elapsed < Duration::from_secs(60) {
        return format!("{:.2}s", elapsed.as_secs_f64());
    }
    let minutes = elapsed.as_secs() / 60;
    let seconds = elapsed.as_secs() % 60;
    format!("{minutes}m {seconds:02}s")
}

#[must_use]
pub(crate) fn format_count(count: usize, noun: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{count} {noun}{suffix}")
}

fn table_widths(rows: &[TableRow]) -> (usize, usize) {
    rows.iter().fold((0, 0), |(label_width, value_width), row| {
        (
            label_width.max(row.label.chars().count()),
            value_width.max(row.value.chars().count()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{TableRow, format_count, format_duration, table_widths};
    use std::time::Duration;

    #[test]
    fn formats_durations_for_terminal_scanning() {
        assert_eq!(format_duration(Duration::from_micros(42)), "42µs");
        assert_eq!(format_duration(Duration::from_millis(380)), "380ms");
        assert_eq!(format_duration(Duration::from_millis(1_250)), "1.25s");
        assert_eq!(format_duration(Duration::from_secs(82)), "1m 22s");
    }

    #[test]
    fn pluralizes_counted_terminal_facts() {
        assert_eq!(format_count(1, "operation"), "1 operation");
        assert_eq!(format_count(2, "operation"), "2 operations");
    }

    #[test]
    fn aligns_manifest_table_columns_from_plain_cell_widths() {
        let rows = [
            TableRow::new("service", "orders", "1.0.0"),
            TableRow::new("contract", "OpenAPI 3.1.2", "3 operations"),
        ];

        assert_eq!(table_widths(&rows), (8, 13));
    }
}
