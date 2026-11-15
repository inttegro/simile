use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Parser)]
#[command(
    name = "simile",
    version,
    about = "Compare with confidence",
    arg_required_else_help = true,
    propagate_version = true
)]
pub struct Cli {
    /// Select human-readable or stable machine-readable output.
    #[arg(
        long,
        global = true,
        value_enum,
        default_value_t,
        env = "SIMILE_OUTPUT"
    )]
    pub output: OutputFormat,

    /// Control ANSI colour in command output.
    #[arg(long, global = true, value_enum, default_value_t, env = "SIMILE_COLOR")]
    pub color: ColorChoice,

    /// Increase diagnostic verbosity. Repeat for more detail.
    #[arg(
        short,
        long,
        global = true,
        action = ArgAction::Count,
        conflicts_with = "quiet"
    )]
    pub verbose: u8,

    /// Suppress non-essential diagnostics.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Compile an OpenAPI contract and optional HCL behavior modules.
    Compile(CompileArgs),
    /// Check the local environment and report actionable problems.
    Doctor,
    /// Verify and summarize a compiled Simile artifact.
    Inspect(InspectArgs),
    /// Print detailed build information.
    Version,
}

#[derive(Debug, Args)]
pub struct CompileArgs {
    /// OpenAPI JSON or YAML contract.
    pub openapi: PathBuf,

    /// HCL behavior module. Repeat to compile multiple modules.
    #[arg(short, long, value_name = "FILE")]
    pub behavior: Vec<PathBuf>,

    /// Write the compiled runtime artifact to this path.
    #[arg(short, long, value_name = "FILE")]
    pub artifact: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    /// Compiled `.similec` artifact.
    pub artifact: PathBuf,
}
