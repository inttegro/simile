//! Compact, chronological rendering for Simile's structured tracing events.

use std::collections::BTreeMap;
use std::fmt;

use time::{OffsetDateTime, UtcOffset};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::registry::LookupSpan;

use super::{EMPHASIS, FAILURE, MUTED, RAIL, SUCCESS, WARNING};

pub(crate) struct TimelineFormatter {
    verbosity: u8,
}

impl TimelineFormatter {
    #[must_use]
    pub(crate) const fn new(verbosity: u8) -> Self {
        Self { verbosity }
    }
}

impl<S, N> FormatEvent<S, N> for TimelineFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut fields = EventFields::default();
        event.record(&mut fields);
        write_prefix(&mut writer)?;

        match event.metadata().target() {
            "simile::http" => render_http(&mut writer, &fields, self.verbosity),
            "simile::proxy" => render_proxy(&mut writer, &fields, self.verbosity),
            _ => render_generic(&mut writer, event.metadata().level(), &fields),
        }
    }
}

fn write_prefix(writer: &mut Writer<'_>) -> fmt::Result {
    let utc = OffsetDateTime::now_utc();
    let now = UtcOffset::current_local_offset().map_or(utc, |offset| utc.to_offset(offset));
    write!(
        writer,
        "{MUTED}{:02}:{:02}:{:02}{MUTED:#} {RAIL}│{RAIL:#} ",
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn render_http(writer: &mut Writer<'_>, fields: &EventFields, verbosity: u8) -> fmt::Result {
    let Some(method) = fields.get("method") else {
        return render_message(writer, SUCCESS, "✓", fields);
    };
    let path = fields.get("path").unwrap_or("-");
    let status = fields.get("status").unwrap_or("-");
    let (status_style, symbol) = status_presentation(status);
    let elapsed = fields
        .get("elapsed_us")
        .and_then(|value| value.parse::<u64>().ok())
        .map(format_micros)
        .unwrap_or_else(|| "-".to_owned());
    let request = display_size(fields.get("request_bytes"));
    let response = display_size(fields.get("response_bytes"));
    let origin = fields.get("origin").unwrap_or("-");
    let user_agent = fields.get("user_agent").unwrap_or("-");

    write!(
        writer,
        "{status_style}{symbol}{status_style:#} {EMPHASIS}{method:<6}{EMPHASIS:#} {path}  {status_style}{status}{status_style:#} {MUTED}{elapsed} · {request} → {response} · {origin} · {user_agent}{MUTED:#}"
    )?;
    if verbosity > 0 {
        let request_type = fields.get("request_type").unwrap_or("-");
        let response_type = fields.get("response_type").unwrap_or("-");
        let details = verbose_http_details(request_type, response_type);
        write!(writer, " {MUTED}{details}{MUTED:#}")?;
    }
    writer.write_char('\n')
}

fn render_proxy(writer: &mut Writer<'_>, fields: &EventFields, verbosity: u8) -> fmt::Result {
    if fields.get("method").is_none() {
        return render_message(writer, SUCCESS, "•", fields);
    }
    let method = fields.get("method").unwrap_or("-");
    let path = fields.get("path").unwrap_or("-");
    let status = fields.get("status").unwrap_or("-");
    let (status_style, symbol) = status_presentation(status);
    let elapsed = fields
        .get("elapsed_us")
        .and_then(|value| value.parse::<u64>().ok())
        .map(format_micros)
        .unwrap_or_else(|| "-".to_owned());
    let direction = fields.get("direction").unwrap_or("proxy");
    let request = display_size(fields.get("request_bytes"));
    let response = display_size(fields.get("response_bytes"));

    write!(
        writer,
        "{status_style}{symbol}{status_style:#} {EMPHASIS}{method:<6}{EMPHASIS:#} {path}  {status_style}{status}{status_style:#} {MUTED}{elapsed} · {request} → {response} · {direction}{MUTED:#}"
    )?;
    if verbosity > 0 {
        let peer = fields.get("peer").unwrap_or("-");
        let details = verbose_proxy_details(peer);
        write!(writer, " {MUTED}{details}{MUTED:#}")?;
    }
    writer.write_char('\n')
}

fn render_generic(writer: &mut Writer<'_>, level: &Level, fields: &EventFields) -> fmt::Result {
    let (style, symbol) = match *level {
        Level::ERROR => (FAILURE, "✗"),
        Level::WARN => (WARNING, "!"),
        Level::INFO => (SUCCESS, "•"),
        Level::DEBUG | Level::TRACE => (MUTED, "·"),
    };
    render_message(writer, style, symbol, fields)
}

fn render_message(
    writer: &mut Writer<'_>,
    style: anstyle::Style,
    symbol: &str,
    fields: &EventFields,
) -> fmt::Result {
    let message = fields.get("message").unwrap_or("event");
    write!(writer, "{style}{symbol}{style:#} {message}")?;
    for (name, value) in fields.iter_without_message() {
        write!(writer, " {MUTED}{name}={value}{MUTED:#}")?;
    }
    writer.write_char('\n')
}

fn status_presentation(status: &str) -> (anstyle::Style, &'static str) {
    match status.parse::<u16>() {
        Ok(200..=399) => (SUCCESS, "✓"),
        Ok(400..=499) => (WARNING, "!"),
        Ok(500..=599) => (FAILURE, "✗"),
        _ => (MUTED, "•"),
    }
}

fn display_size(value: Option<&str>) -> String {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .map(format_bytes)
        .unwrap_or_else(|| "?".to_owned())
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1_000 {
        return format!("{bytes}B");
    }
    if bytes < 1_000_000 {
        return format!("{:.1}kB", bytes as f64 / 1_000.0);
    }
    format!("{:.1}MB", bytes as f64 / 1_000_000.0)
}

fn format_micros(micros: u64) -> String {
    if micros < 1_000 {
        return format!("{micros}µs");
    }
    if micros < 1_000_000 {
        return format!("{}ms", micros / 1_000);
    }
    format!("{:.2}s", micros as f64 / 1_000_000.0)
}

fn verbose_http_details(request_type: &str, response_type: &str) -> String {
    format!("· {request_type} → {response_type}")
}

fn verbose_proxy_details(peer: &str) -> String {
    format!("· peer {peer}")
}

#[derive(Default)]
struct EventFields {
    values: BTreeMap<&'static str, String>,
}

impl EventFields {
    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn iter_without_message(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .iter()
            .filter(|(name, _)| **name != "message")
            .map(|(name, value)| (*name, value.as_str()))
    }

    fn insert(&mut self, field: &Field, value: impl ToString) {
        self.values.insert(field.name(), value.to_string());
    }
}

impl Visit for EventFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.insert(field, format_args!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, value);
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.insert(field, value);
    }
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, format_micros, verbose_http_details, verbose_proxy_details};

    #[test]
    fn formats_timeline_measurements_compactly() {
        assert_eq!(format_bytes(42), "42B");
        assert_eq!(format_bytes(1_240), "1.2kB");
        assert_eq!(format_micros(321), "321µs");
        assert_eq!(format_micros(38_000), "38ms");
    }

    #[test]
    fn keeps_verbose_timeline_details_on_the_event_row() {
        let http = verbose_http_details("application/json", "application/json");
        let proxy = verbose_proxy_details("api.example.test");

        assert_eq!(http, "· application/json → application/json");
        assert_eq!(proxy, "· peer api.example.test");
        assert!(!http.contains('\n'));
        assert!(!proxy.contains('\n'));
    }
}
