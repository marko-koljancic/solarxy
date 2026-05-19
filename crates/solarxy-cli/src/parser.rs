use clap::Parser;
use std::path::PathBuf;

use super::validators::is_valid_model_path;
use solarxy_validate::adapter::{AdapterFormat, AdapterName, FailOn};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[clap(short = 'm',
    long = "model",
    help = "Path to the model file (optional in view mode — drop a file onto the window)",
    value_parser = is_valid_model_path)]
    pub model_path: Option<PathBuf>,
    #[clap(
        short = 'M',
        long = "mode",
        help = "Operation mode: 'view' or 'analyze'",
        default_value = "view"
    )]
    pub mode: OperationMode,
    #[clap(
        short = 'f',
        long = "format",
        help = "Output format: 'text' or 'json' (requires analyze mode)",
        default_value = "text"
    )]
    pub format: OutputFormat,
    #[clap(
        short = 'o',
        long = "output",
        help = "Write analysis report to file (requires analyze mode)"
    )]
    pub output: Option<PathBuf>,
    #[clap(
        long = "config",
        value_name = "PATH",
        help = "Path to a solarxy.toml. When omitted, discovery walks upward from the model's directory until .git/ or filesystem root."
    )]
    pub config: Option<PathBuf>,
    #[clap(
        long = "paths",
        value_name = "GLOB",
        help = "Glob pattern(s) for batch validation (e.g. 'assets/**/*.glb'). Repeatable. Presence of --paths switches dispatch from single-file analyze to the validate orchestrator.",
        num_args = 1..,
    )]
    pub paths: Vec<String>,
    #[clap(
        long = "adapter",
        value_enum,
        default_value_t = AdapterName::Generic,
        help = "Pipeline adapter to format the output."
    )]
    pub adapter: AdapterName,
    #[clap(
        long = "adapter-format",
        value_enum,
        help = "Output format for the chosen adapter. Defaults to the adapter's default (generic→json, github-actions→gha-commands)."
    )]
    pub adapter_format: Option<AdapterFormat>,
    #[clap(
        long = "fail-on",
        value_enum,
        default_value_t = FailOn::Error,
        help = "Exit-code policy: 'error' (fail on errors only), 'warning' (fail on either), 'never' (always exit 0)."
    )]
    pub fail_on: FailOn,
    #[arg(long, help = "Print version and project info")]
    pub about: bool,
    #[arg(long, help = "Check for updates and self-update")]
    pub update: bool,
}

#[derive(Clone, Default, clap::ValueEnum, PartialEq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

impl std::fmt::Debug for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "Text"),
            Self::Json => write!(f, "Json"),
        }
    }
}

#[derive(Clone, clap::ValueEnum, PartialEq)]
pub enum OperationMode {
    View = 0,
    Analyze = 1,
}

impl std::fmt::Debug for OperationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::View => write!(f, "View"),
            Self::Analyze => write!(f, "Analyze"),
        }
    }
}
