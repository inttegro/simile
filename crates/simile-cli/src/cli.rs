use std::path::PathBuf;

use clap::{ArgAction, ArgGroup, Args, Parser, Subcommand, ValueEnum};

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum HeuristicChoice {
    Off,
    Conservative,
    #[default]
    Standard,
    Aggressive,
}

impl From<HeuristicChoice> for simile_heuristics::Policy {
    fn from(value: HeuristicChoice) -> Self {
        match value {
            HeuristicChoice::Off => Self::Off,
            HeuristicChoice::Conservative => Self::Conservative,
            HeuristicChoice::Standard => Self::Standard,
            HeuristicChoice::Aggressive => Self::Aggressive,
        }
    }
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

    /// Show command details and increase diagnostic verbosity. Repeat for more detail.
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
    /// Execute one operation from an OpenAPI contract.
    Run(RunArgs),
    /// Start a configured service emulator.
    Start(StartArgs),
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

    /// Control provider-neutral synthesis inference embedded in the artifact.
    #[arg(long, value_enum, default_value_t, env = "SIMILE_HEURISTICS")]
    pub heuristics: HeuristicChoice,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    /// Compiled `.similec` artifact.
    pub artifact: PathBuf,
}

#[derive(Debug, Args)]
#[command(
    group(
        ArgGroup::new("spec_source")
            .required(true)
            .multiple(false)
            .args(["spec", "spec_url"])
    ),
    group(
        ArgGroup::new("operation_selector")
            .required(true)
            .multiple(false)
            .args(["operation", "operation_id"])
    )
)]
pub struct RunArgs {
    /// OpenAPI JSON or YAML contract on disk.
    #[arg(value_name = "SPEC")]
    pub spec: Option<PathBuf>,

    /// Read the OpenAPI contract from this URL without saving it.
    #[arg(long, value_name = "URL")]
    pub spec_url: Option<String>,

    /// HCL behavior module. Repeat to compile multiple modules.
    #[arg(short, long, value_name = "FILE")]
    pub behavior: Vec<PathBuf>,

    /// Select an operation by its structural `METHOD /path` key.
    #[arg(long, value_name = "METHOD /path")]
    pub operation: Option<String>,

    /// Select an operation by its exact OpenAPI `operationId`.
    #[arg(long, value_name = "ID")]
    pub operation_id: Option<String>,

    /// Read a JSON request object inline, from a file path, or from stdin with `-`.
    #[arg(long, value_name = "JSON|FILE|-")]
    pub request: Option<String>,

    /// Select the request media type when synthesizing a request.
    #[arg(long, value_name = "TYPE", conflicts_with = "request")]
    pub content_type: Option<String>,

    /// Control provider-neutral synthesis inference for this run.
    #[arg(long, value_enum, default_value_t, env = "SIMILE_HEURISTICS")]
    pub heuristics: HeuristicChoice,
}

#[derive(Debug, Args)]
pub struct StartArgs {
    /// Service name declared by the configuration.
    pub service: String,

    /// Runnable service HCL configuration.
    #[arg(long, value_name = "FILE")]
    pub config: PathBuf,

    /// Override the configured listener port for this invocation.
    #[arg(
        long,
        value_name = "PORT",
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub port: Option<u16>,
}
