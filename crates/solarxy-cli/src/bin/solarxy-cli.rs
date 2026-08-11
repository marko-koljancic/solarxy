use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;

use solarxy_cli::parser::{Args, OperationMode, OutputFormat};

#[cfg(feature = "analyzer")]
use solarxy_cli::calc::analyze::ModelAnalyzer;
#[cfg(feature = "analyzer")]
use solarxy_validate::{
    self as validate, ConfigSource, Output as ValidateOutput,
    adapter::{AdapterFormat, AdapterName, FailOn},
};

fn main() -> anyhow::Result<ExitCode> {
    // Standard error, explicitly. The default writer is standard output, which
    // put every log line in the same stream as the report and would put them in
    // the same stream as an image. The rule this release establishes is that
    // standard output is data and standard error is everything else.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "solarxy=info,wgpu_hal=error,wgpu_core=error".into()),
        )
        .init();

    // Parsed rather than `parse()`, because clap exits 2 on a usage error and
    // this command's taxonomy spends 2 on an input that could not be loaded.
    // One is a mistake in the command line and the other is a mistake in the
    // scene, and a build system branches on the difference.
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(e) => {
            // Help and version are not failures: they print to standard output
            // and succeed, which is what every tool does.
            let ok = matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            let _ = e.print();
            return Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            });
        }
    };

    // A subcommand owns the run when one is given. The flat modes below are the
    // shipped surface and keep working untouched.
    if let Some(solarxy_cli::parser::Command::Render(render)) = args.command {
        return Ok(run_render(&render));
    }

    if args.about {
        let version = env!("CARGO_PKG_VERSION");
        let description = env!("CARGO_PKG_DESCRIPTION");
        let repository = env!("CARGO_PKG_REPOSITORY");
        let license = env!("CARGO_PKG_LICENSE");

        println!("Solarxy CLI {version}");
        println!("{description}");
        println!();
        println!("Repository   {repository}");
        println!("License      {license}");
        println!("Contact      https://koljam.com");
        return Ok(ExitCode::SUCCESS);
    }

    if args.list_tui_themes {
        solarxy_cli::tui::theme::print_listing();
        return Ok(ExitCode::SUCCESS);
    }

    if args.update {
        return run_update().map(|()| ExitCode::SUCCESS);
    }

    let model_path = args
        .model_path
        .map(|p| -> anyhow::Result<String> {
            let canonical = fs::canonicalize(&p).context("Failed to canonicalize model path")?;
            Ok(canonical.to_string_lossy().to_string())
        })
        .transpose()?;

    match args.mode {
        OperationMode::View => Ok(exec_gui(model_path.as_deref())),
        OperationMode::Analyze => {
            if !args.paths.is_empty() {
                run_validate(
                    &args.paths,
                    args.config.as_deref(),
                    args.adapter,
                    args.adapter_format,
                    args.fail_on,
                    args.output.as_deref(),
                )
                .map(|code| u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from))
            } else {
                run_analyze(
                    model_path,
                    &args.format,
                    args.output.as_deref(),
                    args.config.as_deref(),
                    args.tui_theme.as_deref(),
                )
                .map(|()| ExitCode::SUCCESS)
            }
        }
    }
}

fn exec_gui(model_path: Option<&str>) -> ExitCode {
    let gui_bin_name = if cfg!(target_os = "windows") {
        "solarxy.exe"
    } else {
        "solarxy"
    };

    let gui_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(gui_bin_name)))
        .filter(|p| p.exists());

    let mut cmd = match gui_path {
        Some(p) => std::process::Command::new(p),
        None => std::process::Command::new("solarxy"),
    };

    if let Some(m) = model_path {
        cmd.arg("--model").arg(m);
    }

    match cmd.status() {
        Ok(status) => status
            .code()
            .and_then(|c| u8::try_from(c).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from),
        Err(e) => {
            eprintln!("Failed to launch solarxy GUI: {e}");
            eprintln!();
            eprintln!("The Solarxy GUI is distributed separately from the CLI:");
            eprintln!("  Linux:   flatpak install flathub dev.koljam.solarxy");
            eprintln!("  macOS:   brew install --cask koljam/solarxy/solarxy");
            eprintln!("  Windows: winget install Koljam.Solarxy");
            eprintln!();
            eprintln!("Or download from https://github.com/marko-koljancic/solarxy/releases");
            ExitCode::from(127)
        }
    }
}

#[cfg(feature = "analyzer")]
fn run_analyze(
    model_path: Option<String>,
    format: &OutputFormat,
    output: Option<&Path>,
    config: Option<&Path>,
    tui_theme: Option<&str>,
) -> anyhow::Result<()> {
    let model_path =
        model_path.ok_or_else(|| anyhow::anyhow!("Model path required for analyze mode"))?;
    let analyzer =
        ModelAnalyzer::new_with_config(&model_path, config).context("Failed to load model")?;
    let report = analyzer.generate_report();

    let rendered = match format {
        OutputFormat::Json => solarxy_core::json::report_to_json(&report)?,
        OutputFormat::Text => report.to_string(),
    };

    if let Some(output_path) = output {
        std::fs::write(output_path, &rendered).context("Failed to write report")?;
        tracing::info!("Report written to {}", output_path.display());
        Ok(())
    } else if *format == OutputFormat::Json && io::stdout().is_terminal() {
        let json_path = std::path::Path::new(&model_path).with_extension("json");
        std::fs::write(&json_path, &rendered).context("Failed to write JSON report")?;
        tracing::info!("Report written to {}", json_path.display());
        Ok(())
    } else if *format == OutputFormat::Json || !io::stdout().is_terminal() {
        print!("{rendered}");
        Ok(())
    } else if let Some(reason) = terminal_too_small() {
        // Decided before the screen is taken, so the notice lands on the
        // normal terminal rather than flashing on an alternate one that is
        // about to be torn down.
        eprintln!("{reason}");
        print!("{rendered}");
        Ok(())
    } else {
        run_analyze_surface(&report, &analyzer, tui_theme)?;
        Ok(())
    }
}

/// Whether this terminal is below the size the analyze surface needs.
///
/// A terminal that will not report its own size is taken at its word rather
/// than refused: the surface is the better experience when it fits, and
/// guessing "too small" would deny it to anyone whose terminal is merely
/// reticent.
#[cfg(feature = "analyzer")]
fn terminal_too_small() -> Option<String> {
    let (width, height) = crossterm::terminal::size().ok()?;
    solarxy_cli::tui::shell::below_floor(width, height)
}

/// Assemble the analyze surface and hand it the terminal.
///
/// The assembly is here rather than behind a library entry point because the
/// pieces are the reusable half of that module tree: a second surface takes
/// the capability model, the theme system and the split tree and supplies its
/// own panels. One opaque call would hide exactly the seam that makes it
/// cheap.
#[cfg(feature = "analyzer")]
fn run_analyze_surface(
    report: &solarxy_core::report::AnalysisReport,
    analyzer: &ModelAnalyzer,
    requested_theme: Option<&str>,
) -> std::io::Result<()> {
    use solarxy_cli::tui;
    use tui::app::App;
    use tui::caps::Capabilities;
    use tui::geometry::{MeshView, ModelView};
    use tui::layout::Preset;
    use tui::prefs::TuiPrefs;
    use tui::theme::ThemeSet;

    let caps = Capabilities::detect();
    let (prefs, notices) = TuiPrefs::load();
    let wanted = requested_theme.or(prefs.theme.as_deref());
    let (theme, theme_notices) = ThemeSet::load().resolve(wanted, caps);
    for notice in notices {
        tracing::warn!("{notice}");
    }
    for notice in theme_notices {
        tracing::warn!("{notice}");
    }

    // A view, not a copy: the analyzer owns these arrays for the whole
    // session, and copying them would throw away the reason the plots cost
    // nothing to draw.
    let model = ModelView {
        meshes: analyzer
            .meshes
            .iter()
            .map(|mesh| MeshView {
                positions: &mesh.positions,
                texcoords: &mesh.texcoords,
                indices: &mesh.indices,
            })
            .collect(),
    };

    let layout = prefs
        .opening_layout()
        .unwrap_or_else(|| Preset::Survey.layout());
    let mut app = App::new(report, &model, layout, theme, caps);
    tui::shell::run(&mut app)
}

#[cfg(not(feature = "analyzer"))]
fn run_analyze(
    _model_path: Option<String>,
    _format: &OutputFormat,
    _output: Option<&Path>,
    _config: Option<&Path>,
    _tui_theme: Option<&str>,
) -> anyhow::Result<()> {
    anyhow::bail!("Analyzer not available: rebuild solarxy-cli with the 'analyzer' feature")
}

#[cfg(feature = "analyzer")]
fn run_validate(
    paths: &[String],
    config_path: Option<&Path>,
    adapter_name: AdapterName,
    adapter_format: Option<AdapterFormat>,
    fail_on: FailOn,
    output: Option<&Path>,
) -> anyhow::Result<i32> {
    let source = match config_path {
        Some(p) => ConfigSource::load(p)?,
        None => ConfigSource::discover(Path::new("."))?,
    };
    let adapter = validate::resolve_adapter(adapter_name);
    let format = adapter_format.unwrap_or_else(|| adapter.default_format());
    let out = ValidateOutput::from_path(output);
    validate::run_validation(paths, source, adapter.as_ref(), format, fail_on, &out)
        .map_err(anyhow::Error::from)
}

#[cfg(not(feature = "analyzer"))]
fn run_validate(
    _paths: &[String],
    _config_path: Option<&Path>,
    _adapter_name: AdapterName,
    _adapter_format: Option<AdapterFormat>,
    _fail_on: FailOn,
    _output: Option<&Path>,
) -> anyhow::Result<i32> {
    anyhow::bail!("Validator not available: rebuild solarxy-cli with the 'analyzer' feature")
}

#[cfg(feature = "updater")]
fn run_update() -> anyhow::Result<()> {
    use axoupdater::AxoUpdater;
    use solarxy_core::install_source::{InstallSource, detect};

    match detect() {
        InstallSource::HomebrewFormula => {
            eprintln!("This Solarxy CLI was installed via Homebrew. Update with:");
            eprintln!("  brew upgrade solarxy-cli");
            return Ok(());
        }
        InstallSource::Flatpak => {
            eprintln!("This Solarxy is running inside Flatpak — update via Flathub:");
            eprintln!("  flatpak update dev.koljam.solarxy");
            return Ok(());
        }
        _ => {}
    }

    let mut updater = AxoUpdater::new_for("solarxy-cli");
    updater.load_receipt()?;
    if updater.run_sync()?.is_some() {
        println!("solarxy-cli has been updated successfully.");
    } else {
        println!("solarxy-cli is already up to date.");
    }
    Ok(())
}

#[cfg(not(feature = "updater"))]
fn run_update() -> anyhow::Result<()> {
    anyhow::bail!("Updater not available: rebuild solarxy-cli with the 'updater' feature")
}

/// Renders, and turns the outcome into an exit code a build system can branch
/// on.
///
/// The taxonomy is the point. A pipeline retries a device that was lost and
/// gives up on a scene that will not parse, and it can only tell them apart if
/// the difference survives all the way out here.
#[cfg(feature = "render")]
fn run_render(args: &solarxy_cli::parser::RenderArgs) -> ExitCode {
    use solarxy_render::{Output, RenderOptions};

    let output = Output::from_path(&args.out);
    // One stream, one datum. Both of these want standard output, and a caller
    // who wants both can redirect one of them.
    if args.json && output == Output::Stdout {
        eprintln!(
            "error: --json and --out - both write to standard output; \
             send one of them elsewhere"
        );
        return ExitCode::from(1);
    }

    let (width, height) = match args.res.as_deref().map(parse_resolution) {
        Some(Ok(wh)) => (Some(wh.0), Some(wh.1)),
        Some(Err(message)) => {
            eprintln!("error: {message}");
            return ExitCode::from(1);
        }
        None => (None, None),
    };

    // The flag reaches the engine's own cancellation closure, so an interrupt
    // stops between cook passes and between tiles rather than at the end.
    // Nothing partial is left behind: the image is written in one call after
    // the last tile, so a run that stops early never creates the file.
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let flag = std::sync::Arc::clone(&cancel);
        if let Err(e) = ctrlc::set_handler(move || {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }) {
            tracing::warn!("interrupt handling unavailable: {e}");
        }
    }

    let opts = RenderOptions {
        output: Some(output),
        render_node: args.render_node.clone(),
        width,
        height,
        samples: args.spp,
        bounces: args.bounces,
        denoise: if args.denoise {
            Some(true)
        } else if args.no_denoise {
            Some(false)
        } else {
            None
        },
        engine: args.engine.map(|e| match e {
            solarxy_cli::parser::RenderEngineArg::PathTraced => {
                solarxy_render::RenderEngine::PathTraced
            }
            solarxy_cli::parser::RenderEngineArg::Raster => solarxy_render::RenderEngine::Raster,
        }),
        seed: args.seed,
        aovs: args
            .aov
            .iter()
            .map(|a| match a {
                solarxy_cli::parser::AovArg::Albedo => solarxy_render::AovKind::Albedo,
                solarxy_cli::parser::AovArg::Normal => solarxy_render::AovKind::Normal,
                solarxy_cli::parser::AovArg::Depth => solarxy_render::AovKind::Depth,
            })
            .collect(),
        exr_space: args.exr_space.map(|s| match s {
            solarxy_cli::parser::ExrSpaceArg::SceneLinear => solarxy_render::ExrSpace::SceneLinear,
            solarxy_cli::parser::ExrSpaceArg::Display => solarxy_render::ExrSpace::Display,
        }),
        cancel: Some(cancel),
    };

    // Standard error, and one line: the sink a build system reads. It rewrites
    // in place on a terminal and writes once per step anywhere else, which is
    // the difference between a live readout and a log worth reading later.
    let mut sink = solarxy_cli::render_sink::PlainSink::new(
        std::io::stderr(),
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
    );
    let mut progress = |p: solarxy_render::RenderProgress| sink.report(&p);

    match solarxy_render::run_render(&args.input, &opts, &mut progress) {
        Ok(outcome) => {
            for warning in &outcome.report.warnings {
                tracing::warn!("{warning}");
            }
            if args.json {
                match outcome.report.to_json() {
                    Ok(line) => println!("{line}"),
                    Err(e) => {
                        eprintln!("error: the result could not be serialized: {e}");
                        return ExitCode::from(7);
                    }
                }
            } else {
                tracing::info!("rendered {}", outcome.report.output);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(exit_code_for(&e))
        }
    }
}

/// The exit taxonomy.
///
/// Eight codes, each naming a failure class rather than a place in the code.
/// Zero and one are what every tool means by them; the rest exist because a
/// render farm treats them differently.
#[cfg(feature = "render")]
fn exit_code_for(error: &solarxy_render::RenderError) -> u8 {
    use solarxy_render::RenderError as E;
    match error {
        E::InputMissing(_)
        | E::InputUnreadable { .. }
        | E::InputInvalid { .. }
        | E::InputUnsupported { .. } => 2,
        // A flag that cannot take effect is a mistake in the invocation, which
        // is what code one means, and it is decided before anything is read.
        E::OptionIneffective(_) => 1,
        E::Cook(_) | E::NoRenderNode | E::AmbiguousRenderNode(_) | E::RenderNode(_) => 3,
        E::NoAdapter => 4,
        E::Device(_) | E::DeviceLost => 5,
        E::Cancelled => 6,
        E::Encode(_) | E::OutputUnwritable { .. } => 7,
    }
}

/// `WIDTHxHEIGHT`, as a command line spells a resolution.
#[cfg(feature = "render")]
fn parse_resolution(text: &str) -> Result<(u32, u32), String> {
    let (w, h) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("--res wants WIDTHxHEIGHT, got '{text}'"))?;
    let w = w
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("--res width is not a number: '{w}'"))?;
    let h = h
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("--res height is not a number: '{h}'"))?;
    Ok((w, h))
}

/// The stub arm, following the pattern the other optional surfaces use: a build
/// without the feature explains itself rather than failing to parse an argument
/// it does not know.
#[cfg(not(feature = "render"))]
fn run_render(_args: &solarxy_cli::parser::RenderArgs) -> ExitCode {
    eprintln!(
        "error: rendering is not available: rebuild solarxy-cli with the \
         'render' feature"
    );
    ExitCode::from(1)
}
