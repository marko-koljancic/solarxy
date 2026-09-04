use clap::Parser;
use std::path::PathBuf;

use super::validators::is_valid_model_path;
use solarxy_validate::adapter::{AdapterFormat, AdapterName, FailOn};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// The subcommand, when one is given.
    ///
    /// Optional, and beside the flat arguments rather than replacing them: the
    /// analyze and view modes are a shipped surface with users, and turning
    /// them into subcommands would break every invocation in every pipeline
    /// that already runs them. New surfaces are subcommands; the old ones stay
    /// where they are.
    #[command(subcommand)]
    pub command: Option<Command>,

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
    #[clap(
        long = "tui-theme",
        value_name = "NAME",
        global = true,
        help = "Terminal theme for the analyze surface and the render dashboard; applies at 256-colour and truecolor terminals"
    )]
    pub tui_theme: Option<String>,
    #[clap(
        long = "list-tui-themes",
        global = true,
        help = "List the terminal themes this build can find, with a swatch of each"
    )]
    pub list_tui_themes: bool,
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

/// The subcommands. One so far.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Render a scene or a model to an image file.
    Render(RenderArgs),
}

/// `solarxy-cli render`.
///
/// Every option that has a counterpart on the render node is optional and
/// overrides it. The node stays authoritative and the flags are a convenience,
/// which is what stops a scene and a command line becoming two descriptions of
/// one render that disagree.
// A command line is made of flags, and a flag is a bool. Grouping them into
// sub-structs to satisfy a count would change what `--help` prints and what a
// caller writes, which is the wrong way round: the lint is about a struct
// nobody types, and this is one everybody does.
#[allow(clippy::struct_excessive_bools)]
#[derive(clap::Args, Debug)]
pub struct RenderArgs {
    /// A `.slxy` scene, or a model file.
    #[arg(value_parser = crate::validators::is_valid_render_input)]
    pub input: PathBuf,

    /// Where the image goes. `-` writes it to standard output.
    #[arg(short, long)]
    pub out: PathBuf,

    /// The render node to use, by name, when a scene has more than one.
    #[arg(long)]
    pub render_node: Option<String>,

    /// Output size as `WIDTHxHEIGHT`.
    #[arg(long, value_name = "WxH")]
    pub res: Option<String>,

    /// Samples per pixel. Path-traced renders only.
    #[arg(long)]
    pub spp: Option<u32>,

    /// Light bounces per path. Path-traced renders only.
    #[arg(long)]
    pub bounces: Option<u32>,

    /// Which renderer draws it.
    #[arg(long, value_enum)]
    pub engine: Option<RenderEngineArg>,

    /// Filter the image after rendering. Path-traced renders only.
    #[arg(long)]
    pub denoise: bool,

    /// Turn the filter off, overriding the scene.
    #[arg(long, conflicts_with = "denoise")]
    pub no_denoise: bool,

    /// Fix the sampling sequence, so two runs of the same scene on the same
    /// device produce the same image.
    #[arg(long)]
    pub seed: Option<u32>,

    /// Auxiliary passes to write beside the image, as 32-bit float EXR.
    ///
    /// Path-traced renders only, and refused rather than ignored when the
    /// engine cannot produce them.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "LIST")]
    pub aov: Vec<AovArg>,

    /// Which space a floating-point image is written in. `.exr` output only.
    #[arg(long, value_enum)]
    pub exr_space: Option<ExrSpaceArg>,

    /// Write a machine-readable result to standard output.
    #[arg(long)]
    pub json: bool,

    /// Show the render on a terminal dashboard instead of one line.
    ///
    /// Paints on standard error, so this and `--json` can be given together.
    /// Falls back to the plain line when standard error is not a terminal or
    /// is too small for the surface.
    #[arg(long)]
    pub tui: bool,

    /// Show the render in a window as it converges.
    ///
    /// A build feature, off by default. A build without it says so rather than
    /// refusing to parse the flag.
    #[arg(long)]
    pub watch: bool,
}

/// An auxiliary pass, as a flag value.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
#[clap(rename_all = "kebab-case")]
pub enum AovArg {
    Albedo,
    Normal,
    Depth,
}

/// Which floats a float image carries.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
#[clap(rename_all = "kebab-case")]
pub enum ExrSpaceArg {
    /// Light as the scene has it, with no exposure, tone map or grade applied.
    SceneLinear,
    /// The finished look, without the quantization a screen would impose.
    Display,
}

/// The engine choice as a flag. Mirrors what the render node declares, spelled
/// the way a command line spells things.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
#[clap(rename_all = "kebab-case")]
pub enum RenderEngineArg {
    Raster,
    PathTraced,
}
