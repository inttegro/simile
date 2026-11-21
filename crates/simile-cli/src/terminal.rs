use std::fmt;
use std::io::{self, Write};

use anstream::AutoStream;
use anstyle::{AnsiColor, Style};
use serde::Serialize;
use serde_json::Value;

use crate::cli::{ColorChoice, OutputFormat};
use crate::error::{Error, Result};

const JSON_KEY: Style = AnsiColor::Blue.on_default();
const JSON_STRING: Style = AnsiColor::Green.on_default();
const JSON_NUMBER: Style = AnsiColor::Cyan.on_default();
const JSON_BOOLEAN: Style = AnsiColor::Yellow.on_default();
const JSON_NULL: Style = AnsiColor::BrightBlack.on_default();

#[derive(Clone, Copy, Debug)]
pub struct Terminal {
    output: OutputFormat,
    color: anstream::ColorChoice,
    verbose: bool,
    quiet: bool,
}

impl Terminal {
    #[must_use]
    pub fn new(output: OutputFormat, color: ColorChoice, verbose: bool, quiet: bool) -> Self {
        let color = match color {
            ColorChoice::Auto => anstream::ColorChoice::Auto,
            ColorChoice::Always => anstream::ColorChoice::Always,
            ColorChoice::Never => anstream::ColorChoice::Never,
        };
        Self {
            output,
            color,
            verbose,
            quiet,
        }
    }

    #[must_use]
    pub const fn output(&self) -> OutputFormat {
        self.output
    }

    #[must_use]
    pub const fn is_verbose(&self) -> bool {
        self.verbose
    }

    pub fn line(&self, arguments: fmt::Arguments<'_>) -> Result<()> {
        let stdout = io::stdout();
        let mut stream = AutoStream::new(stdout.lock(), self.color);
        stream
            .write_fmt(arguments)
            .and_then(|()| stream.write_all(b"\n"))
            .map_err(Error::Output)
    }

    pub fn status_line(&self, arguments: fmt::Arguments<'_>) -> Result<()> {
        if self.quiet {
            return Ok(());
        }
        let stderr = io::stderr();
        let mut stream = AutoStream::new(stderr.lock(), self.color);
        stream
            .write_fmt(arguments)
            .and_then(|()| stream.write_all(b"\n"))
            .map_err(Error::Output)
    }

    pub fn json<T>(&self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        let stdout = io::stdout();
        let mut stream = AutoStream::new(stdout.lock(), anstream::ColorChoice::Never);
        serde_json::to_writer_pretty(&mut stream, value).map_err(Error::Serialize)?;
        stream.write_all(b"\n").map_err(Error::Output)
    }

    pub fn pretty_json(&self, value: Option<&Value>) -> Result<()> {
        let mut rendered = String::new();
        render_json(value.unwrap_or(&Value::Null), 0, &mut rendered)?;
        self.line(format_args!("{rendered}"))
    }

    pub fn error(&self, error: &Error) -> io::Result<()> {
        let stderr = io::stderr();
        let mut stream = AutoStream::new(stderr.lock(), self.color);
        writeln!(stream, "{error}")
    }
}

fn render_json(value: &Value, depth: usize, rendered: &mut String) -> Result<()> {
    match value {
        Value::Null => rendered.push_str(&format!("{JSON_NULL}null{JSON_NULL:#}")),
        Value::Bool(value) => {
            rendered.push_str(&format!("{JSON_BOOLEAN}{value}{JSON_BOOLEAN:#}"));
        }
        Value::Number(value) => {
            rendered.push_str(&format!("{JSON_NUMBER}{value}{JSON_NUMBER:#}"));
        }
        Value::String(value) => {
            let value = serde_json::to_string(value).map_err(Error::Serialize)?;
            rendered.push_str(&format!("{JSON_STRING}{value}{JSON_STRING:#}"));
        }
        Value::Array(values) => {
            rendered.push('[');
            if !values.is_empty() {
                rendered.push('\n');
                for (index, value) in values.iter().enumerate() {
                    indent(rendered, depth + 1);
                    render_json(value, depth + 1, rendered)?;
                    if index + 1 != values.len() {
                        rendered.push(',');
                    }
                    rendered.push('\n');
                }
                indent(rendered, depth);
            }
            rendered.push(']');
        }
        Value::Object(values) => {
            rendered.push('{');
            if !values.is_empty() {
                rendered.push('\n');
                for (index, (key, value)) in values.iter().enumerate() {
                    indent(rendered, depth + 1);
                    let key = serde_json::to_string(key).map_err(Error::Serialize)?;
                    rendered.push_str(&format!("{JSON_KEY}{key}{JSON_KEY:#}: "));
                    render_json(value, depth + 1, rendered)?;
                    if index + 1 != values.len() {
                        rendered.push(',');
                    }
                    rendered.push('\n');
                }
                indent(rendered, depth);
            }
            rendered.push('}');
        }
    }
    Ok(())
}

fn indent(rendered: &mut String, depth: usize) {
    rendered.push_str(&"  ".repeat(depth));
}
