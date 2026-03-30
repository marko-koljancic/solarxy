use clap::Parser;
use std::path::PathBuf;
use super::validators::is_valid_model_path;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[clap(short = 'm', 
    long = "model", 
    required = true, 
    help = "Path to the model file", 
    value_parser = is_valid_model_path)]
    /// Path to the model file to be loaded or processed.
    /// This field specifies the filesystem location of the model that will be used
    /// by the application. The path can be either relative or absolute.
    pub model_path: PathBuf,
    #[clap(
        short = 'o',
        long = "mode",
        help = "Operation mode: 'view' or 'analyze'",
        default_value = "view"
    )]
    /// The operation mode that determines how the application will run with the provided model.
    /// - 'view': Launches a viewer to display the model visually.
    /// - 'analyze': Performs analysis on the model and displays results in a terminal interface.
    pub mode: OperationMode,
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
