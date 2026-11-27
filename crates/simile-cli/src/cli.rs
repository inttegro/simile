use std::ffi::OsString;
use std::net::IpAddr;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ContractFormatChoice {
    #[default]
    Auto,
    #[value(name = "openapi")]
    OpenApi,
    Smithy,
    #[value(name = "typescript")]
    TypeScript,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ReportingChoice {
    Automatic,
    Off,
}

impl From<ContractFormatChoice> for simile_core::ops::ContractFormat {
    fn from(value: ContractFormatChoice) -> Self {
        match value {
            ContractFormatChoice::Auto => Self::Auto,
            ContractFormatChoice::OpenApi => Self::OpenApi,
            ContractFormatChoice::Smithy => Self::Smithy,
            ContractFormatChoice::TypeScript => Self::TypeScript,
        }
    }
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
    /// Audit provider fidelity and explain every remaining contract gap.
    Audit(AuditArgs),
    /// Compile an OpenAPI, Smithy, or semantic TypeScript contract with optional HCL modules.
    Compile(CompileArgs),
    /// Open the interactive workspace for registered local services.
    Console(ConsoleArgs),
    /// Author and exercise a service in the browser.
    Playground(PlaygroundArgs),
    /// Replay one or more executable examples as a conformance suite.
    Conformance(ConformanceArgs),
    /// Check the local environment and report actionable problems.
    Doctor,
    /// Explain the current Simfile or the public semantics of a compiled service.
    Describe(DescribeArgs),
    /// Verify and summarize a compiled Simile artifact.
    Inspect(InspectArgs),
    /// Execute one operation from an OpenAPI, Smithy, or semantic TypeScript contract.
    Run(RunArgs),
    /// Re-run and verify a self-contained reproduction bundle.
    Replay(ReplayArgs),
    /// Prepare, inspect, verify, or send one reproducible issue report.
    Report(ReportArgs),
    /// List prepared reports.
    Reports(ReportsArgs),
    /// Inspect or change automatic reporting consent for a service profile.
    Reporting(ReportingArgs),
    /// Compile and register a service for later use.
    Register(RegisterArgs),
    /// Permanently remove a registered service and its local data.
    Remove(RemoveArgs),
    /// Set up one consumer profile for a registered service.
    Setup(SetupArgs),
    /// List every profile configured for a registered service.
    Profiles(ProfilesArgs),
    /// Inspect one service profile without revealing its credentials.
    Profile(ProfileArgs),
    /// Pass local application traffic through an external service and optionally learn from it.
    Proxy(ProxyArgs),
    /// Start a configured service emulator.
    Start(StartArgs),
    /// Run a test command against a supervised service emulator.
    Test(TestArgs),
    /// Inspect persisted resources and causal history.
    State(StateArgs),
    /// Reload and recompile a registered service.
    Update(UpdateArgs),
    /// Select the default profile for CLI-originated work.
    Use(UseArgs),
    /// Print detailed build information.
    Version,
}

#[derive(Debug, Args)]
pub struct ConsoleArgs {
    /// Registered service to select when the console opens.
    pub service: Option<String>,

    /// Open the console in the system browser instead of the terminal.
    #[arg(long)]
    pub web: bool,
}

#[derive(Debug, Args)]
pub struct PlaygroundArgs {
    /// Loopback port to use.
    #[arg(long, default_value_t = 4243, value_parser = clap::value_parser!(u16).range(1..))]
    pub port: u16,

    /// Print the playground URL without opening the system browser.
    #[arg(long)]
    pub no_open: bool,
}

#[derive(Debug, Args)]
pub struct AuditArgs {
    /// Compiled `.similec` artifact to audit.
    pub artifact: PathBuf,

    /// Return failure when the report contains warnings as well as errors.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, Args)]
pub struct ConformanceArgs {
    /// Reproduction bundle or directory containing bundles. Repeat as needed.
    #[arg(required = true, value_name = "BUNDLE_OR_SUITE")]
    pub bundles: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub struct DescribeArgs {
    #[command(subcommand)]
    pub target: Option<DescribeTarget>,
}

#[derive(Debug, Subcommand)]
pub enum DescribeTarget {
    /// Describe a registered service or a transient service configuration.
    Service(DescribeServiceArgs),
}

#[derive(Debug, Args)]
pub struct DescribeServiceArgs {
    /// Registered service name. Omit to describe `./Simfile`.
    pub service: Option<String>,

    /// Describe this HCL configuration file or URL instead of the registry entry or `./Simfile`.
    #[arg(long, value_name = "URL_OR_FILE")]
    pub config: Option<String>,
}

#[derive(Debug, Args)]
pub struct CompileArgs {
    /// Contract file, or a directory containing semantic TypeScript declarations.
    #[arg(value_name = "CONTRACT")]
    pub contract: PathBuf,

    /// Contract source language. By default Simile infers it from the path.
    #[arg(long, value_enum, default_value_t)]
    pub contract_format: ContractFormatChoice,

    /// HCL module. Repeat to compile multiple modules.
    #[arg(
        short = 'm',
        long = "module",
        alias = "behavior",
        short_alias = 'b',
        value_name = "FILE"
    )]
    pub module: Vec<PathBuf>,

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
    /// Contract file, or a semantic TypeScript declaration entrypoint or directory.
    #[arg(value_name = "CONTRACT")]
    pub spec: Option<PathBuf>,

    /// Read a self-contained contract from this URL without saving it.
    #[arg(long, value_name = "URL")]
    pub spec_url: Option<String>,

    /// Contract source language. Local paths are inferred; remote sources default to OpenAPI.
    #[arg(long, value_enum, default_value_t)]
    pub contract_format: ContractFormatChoice,

    /// HCL module. Repeat to compile multiple modules.
    #[arg(
        short = 'm',
        long = "module",
        alias = "behavior",
        short_alias = 'b',
        value_name = "FILE"
    )]
    pub module: Vec<PathBuf>,

    /// Select an operation by its structural `METHOD /path` key.
    #[arg(long, value_name = "METHOD /path")]
    pub operation: Option<String>,

    /// Select an operation by its exact contract `operationId`.
    #[arg(long, value_name = "ID")]
    pub operation_id: Option<String>,

    /// Select one named public outcome for this invocation.
    #[arg(long, value_name = "NAME", conflicts_with = "scenario")]
    pub outcome: Option<String>,

    /// Activate one opaque public scenario for this invocation.
    #[arg(long, value_name = "NAME", conflicts_with = "outcome")]
    pub scenario: Option<String>,

    /// Read a JSON request object inline, from a file path, or from stdin with `-`.
    #[arg(long, value_name = "JSON|FILE|-")]
    pub request: Option<String>,

    /// Select the request media type when synthesizing a request.
    #[arg(long, value_name = "TYPE", conflicts_with = "request")]
    pub content_type: Option<String>,

    /// Control provider-neutral synthesis inference for this run.
    #[arg(long, value_enum, default_value_t, env = "SIMILE_HEURISTICS")]
    pub heuristics: HeuristicChoice,

    /// Reproduce request and response synthesis exactly.
    #[arg(long, value_name = "U64")]
    pub seed: Option<u64>,

    /// Write a self-contained, replayable execution bundle to this directory.
    #[arg(long, value_name = "DIR")]
    pub bundle: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ReplayArgs {
    /// Directory produced by `simile run --bundle`.
    pub bundle: PathBuf,
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// Registered service name when preparing a report, or an existing `rpt_…` identifier.
    pub target: String,

    /// Reproduction bundle that demonstrates a newly reported inconsistency.
    #[arg(long, value_name = "DIR")]
    pub bundle: Option<PathBuf>,

    /// Profile whose consent and contact should accompany the report.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,

    /// Short, concrete summary of the observed inconsistency.
    #[arg(long, value_name = "TEXT")]
    pub title: Option<String>,

    /// Additional context about the expected behavior.
    #[arg(long, value_name = "TEXT")]
    pub description: Option<String>,

    /// One-time contact email for clarification and resolution updates.
    #[arg(long, value_name = "EMAIL", requires = "send")]
    pub contact: Option<String>,

    /// Submit the report after verification and upgrade preflight.
    #[arg(long)]
    pub send: bool,

    /// Submit with the captured versions after showing available upgrades.
    #[arg(long, requires = "send")]
    pub send_current: bool,
}

#[derive(Debug, Args)]
pub struct ReportsArgs {
    /// Limit the list to one registered service.
    pub service: Option<String>,
}

#[derive(Debug, Args)]
pub struct ReportingArgs {
    /// Registered service name.
    pub service: String,

    /// Profile to configure. The current profile is used when omitted.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,

    /// Enable automatic verified reports or turn reporting off.
    #[arg(long, value_enum)]
    pub mode: Option<ReportingChoice>,

    /// Contact email for clarification and resolution updates.
    #[arg(long, value_name = "EMAIL", requires = "mode")]
    pub contact: Option<String>,
}

#[derive(Debug, Args)]
pub struct StartArgs {
    /// Registered service name. Omit to run `./Simfile`.
    pub service: Option<String>,

    /// Run directly from this HCL configuration instead of the registry or `./Simfile`.
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Override the configured listener address for this invocation.
    #[arg(long, value_name = "IP")]
    pub host: Option<IpAddr>,

    /// Override the configured listener port for this invocation.
    #[arg(
        long,
        value_name = "PORT",
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub port: Option<u16>,

    /// Activate one opaque public scenario for the running emulator.
    #[arg(long, value_name = "NAME")]
    pub scenario: Option<String>,

    /// Enable one bounded stochastic environmental-fault profile.
    #[arg(long, value_name = "NAME")]
    pub fault_profile: Option<String>,
}

#[derive(Debug, Args)]
pub struct TestArgs {
    /// Registered service name. Omit to test against `./Simfile`.
    pub service: Option<String>,

    /// Run directly from this HCL configuration instead of the registry or `./Simfile`.
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Inject this registered caller profile into the test command.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,

    /// Use a fixed listener port instead of an available ephemeral port.
    #[arg(
        long,
        value_name = "PORT",
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub port: Option<u16>,

    /// Activate one opaque public scenario for the test run.
    #[arg(long, value_name = "NAME")]
    pub scenario: Option<String>,

    /// Enable one bounded stochastic environmental-fault profile.
    #[arg(long, value_name = "NAME")]
    pub fault_profile: Option<String>,

    /// Test command and arguments. Precede the command with `--`.
    #[arg(last = true, required = true, num_args = 1.., value_name = "COMMAND")]
    pub command: Vec<OsString>,
}

#[derive(Debug, Args)]
pub struct ProxyArgs {
    /// Local name for the external service being proxied.
    pub service: String,

    /// External service origin that receives proxied API calls.
    #[arg(long = "to", value_name = "URL")]
    pub upstream: String,

    /// Record exchanges and generate a draft Simfile and OpenAPI contract on shutdown.
    #[arg(long)]
    pub learn: bool,

    /// Public origin already routed to this process; enables the derived webhook relay URL.
    #[arg(long, value_name = "URL")]
    pub public_url: Option<String>,

    /// Forward received webhooks to this application endpoint.
    #[arg(long, value_name = "URL", requires = "public_url")]
    pub webhook_destination: Option<String>,

    /// Listener address. Defaults to loopback for local development.
    #[arg(long, value_name = "IP")]
    pub host: Option<IpAddr>,

    /// Listener port. Defaults to 8484.
    #[arg(
        long,
        value_name = "PORT",
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub port: Option<u16>,
}

#[derive(Debug, Args)]
pub struct StateArgs {
    /// Registered service name.
    pub service: String,

    /// Restrict events to one resource such as `db.orders`.
    #[arg(long, value_name = "RESOURCE")]
    pub resource: Option<String>,

    /// Restrict events to one causal parent identifier.
    #[arg(long = "caused-by", value_name = "ID")]
    pub causation_id: Option<String>,

    /// Restrict events to one end-to-end correlation identifier.
    #[arg(long = "correlation", value_name = "ID")]
    pub correlation_id: Option<String>,

    /// Return events after this journal sequence.
    #[arg(long, default_value_t = 0)]
    pub after: u64,

    /// Maximum number of events to return.
    #[arg(long, default_value_t = 100, value_parser = parse_state_limit)]
    pub limit: usize,

    /// Include stored values. Journal inspection is payload-free by default.
    #[arg(long)]
    pub values: bool,
}

fn parse_state_limit(value: &str) -> Result<usize, String> {
    let limit = value
        .parse::<usize>()
        .map_err(|_| "limit must be an integer from 1 through 1000".to_owned())?;
    (1..=1000)
        .contains(&limit)
        .then_some(limit)
        .ok_or_else(|| "limit must be from 1 through 1000".to_owned())
}

#[derive(Debug, Args)]
pub struct RegisterArgs {
    /// Service name. Omit to use the name declared by the configuration.
    pub service: Option<String>,

    /// Runnable service HCL configuration file or URL. Defaults to `./Simfile`.
    #[arg(long, value_name = "URL_OR_FILE")]
    pub config: Option<String>,

    /// Replace the current registration when the service already exists.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    /// Registered service name.
    pub service: String,

    /// Principal to provision. Optional when the service declares one principal setup.
    #[arg(long, value_name = "NAME")]
    pub principal: Option<String>,

    /// Durable profile name. Simile generates one when omitted.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,

    /// Supply one collected setup value as `NAME=VALUE`. Repeat as needed.
    #[arg(long = "set", value_name = "NAME=VALUE")]
    pub values: Vec<String>,

    /// Select this profile as the default after setup.
    #[arg(long = "use")]
    pub select: bool,

    /// Enable automatic verified reports or turn reporting off for the created profile.
    #[arg(long, value_enum)]
    pub reporting: Option<ReportingChoice>,

    /// Contact email for clarification and resolution updates.
    #[arg(long, value_name = "EMAIL", requires = "reporting")]
    pub report_contact: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProfilesArgs {
    /// Registered service name.
    pub service: String,
}

#[derive(Debug, Args)]
pub struct ProfileArgs {
    /// Registered service name.
    pub service: String,

    /// Profile name.
    pub profile: String,
}

#[derive(Debug, Args)]
pub struct UseArgs {
    /// Registered service name.
    pub service: String,

    /// Profile to use by default.
    #[arg(long, value_name = "NAME")]
    pub profile: String,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Registered service name.
    pub service: String,

    /// Repeat the service name to confirm permanent deletion.
    #[arg(long, value_name = "SERVICE", required = true)]
    pub confirm: String,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Registered service name.
    pub service: String,

    /// Replace the service configuration from this HCL file or URL.
    #[arg(long, value_name = "URL_OR_FILE")]
    pub config: Option<String>,
}
