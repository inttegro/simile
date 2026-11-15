use std::fmt;
use std::io::{self, Write};

use anstream::AutoStream;
use serde::Serialize;

use crate::cli::{ColorChoice, OutputFormat};
use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug)]
pub struct Terminal {
    output: OutputFormat,
    color: anstream::ColorChoice,
}

impl Terminal {
    #[must_use]
    pub fn new(output: OutputFormat, color: ColorChoice) -> Self {
        let color = match color {
            ColorChoice::Auto => anstream::ColorChoice::Auto,
            ColorChoice::Always => anstream::ColorChoice::Always,
            ColorChoice::Never => anstream::ColorChoice::Never,
        };
        Self { output, color }
    }

    #[must_use]
    pub const fn output(&self) -> OutputFormat {
        self.output
    }

    pub fn line(&self, arguments: fmt::Arguments<'_>) -> Result<()> {
        let stdout = io::stdout();
        let mut stream = AutoStream::new(stdout.lock(), self.color);
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

    pub fn error(&self, error: &Error) -> io::Result<()> {
        let stderr = io::stderr();
        let mut stream = AutoStream::new(stderr.lock(), self.color);
        writeln!(stream, "{error}")
    }
}
