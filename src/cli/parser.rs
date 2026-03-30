use clap::Parser;
use std::path::PathBuf;

use super::validators::is_valid_model_path;

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
        short = 'o',
        long = "output",
        help = "Write analysis report to file (requires analyze mode)"
    )]
    pub output: Option<PathBuf>,
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
