//! The render command's contract with whatever runs it.
//!
//! Run against the built binary rather than against a function, because the two
//! things under test here only exist there: the exit code the process returns,
//! and which stream each byte went to. A test that called a library function
//! could not observe either.
//!
//! Following the validation smoke suite, which established this shape.
//!
//! # What is not here
//!
//! Six of the eight exit codes are provoked deliberately: zero and one, two for
//! an input that cannot be read, three for a scene the document itself makes
//! unrenderable, six for an interrupt, and seven for an output that cannot be
//! written.
//!
//! Four, no adapter, and five, a lost device, are not. Both need a machine
//! whose GPU behaves in a particular way, and a test that provokes one by
//! breaking the machine is worse than the gap. Four is nonetheless exercised
//! every time this suite runs somewhere without a GPU, which is why several
//! tests below carry an arm for it rather than failing there; five is exercised
//! by hand and recorded.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels below the workspace root")
        .to_path_buf()
}

/// The binary under test, as cargo built it beside this test.
fn binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("the test binary knows where it is");
    path.pop(); // the deps directory
    path.pop();
    path.join(if cfg!(windows) {
        "solarxy-cli.exe"
    } else {
        "solarxy-cli"
    })
}

fn render(args: &[&str]) -> Option<Output> {
    let bin = binary();
    if !bin.exists() {
        // `cargo test -p solarxy-cli` builds the binary, but a filtered run of
        // this file alone may not have. Skipping beats failing on a build
        // artifact's absence.
        eprintln!("skipping: {} has not been built", bin.display());
        return None;
    }
    let mut cmd = Command::new(bin);
    cmd.current_dir(repo_root()).arg("render").args(args);
    Some(cmd.output().expect("the binary runs"))
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

/// An input that is not there exits with the unloadable-input code.
#[test]
fn a_missing_input_exits_two() {
    let Some(out) = render(&["definitely-not-here.slxy", "--out", "/dev/null"]) else {
        return;
    };
    // Two, not one. The command line was well formed and the file named in it
    // was not there, which is a failure of the input rather than of the
    // invocation, and a build system retries those differently.
    assert_eq!(
        code(&out),
        2,
        "stderr said: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "a failure wrote to standard output");
}

/// A file of a kind this does not render is refused by kind.
#[test]
fn an_unrenderable_kind_is_refused() {
    let dir = std::env::temp_dir().join("solarxy-render-contract");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let odd = dir.join("notes.txt");
    std::fs::write(&odd, b"not a model").expect("the file writes");

    let Some(out) = render(&[odd.to_str().expect("utf8 path"), "--out", "/dev/null"]) else {
        return;
    };
    // One: the command line asked for something the command does not do, which
    // it can tell before opening anything.
    assert_eq!(
        code(&out),
        1,
        "stderr said: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "a failure put bytes on standard output: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    let _ = std::fs::remove_file(&odd);
}

/// Asking for the report and the image on the same stream is a usage error.
///
/// The two cannot share it, and the failure has to arrive before the render
/// rather than after minutes of work.
#[test]
fn the_report_and_the_image_cannot_share_standard_output() {
    let model = repo_root().join("res/models/armadillo.obj");
    let Some(out) = render(&[model.to_str().expect("utf8 path"), "--out", "-", "--json"]) else {
        return;
    };
    assert_eq!(
        code(&out),
        1,
        "expected a usage error; stderr said: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "the refusal wrote to standard output"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("standard output"),
        "the message did not explain the clash"
    );
}

/// A malformed resolution is a usage error, not a render at some other size.
#[test]
fn a_malformed_resolution_is_a_usage_error() {
    let model = repo_root().join("res/models/armadillo.obj");
    let Some(out) = render(&[
        model.to_str().expect("utf8 path"),
        "--out",
        "/dev/null",
        "--res",
        "not-a-size",
    ]) else {
        return;
    };
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
}

/// The output path is required, so nothing is ever written somewhere nobody
/// named.
#[test]
fn an_output_path_is_required() {
    let model = repo_root().join("res/models/armadillo.obj");
    let Some(out) = render(&[model.to_str().expect("utf8 path")]) else {
        return;
    };
    assert_eq!(code(&out), 1, "a missing required flag is a usage error");
    assert!(out.stdout.is_empty());
}

/// A pass the chosen renderer cannot produce is refused, not skipped.
///
/// **Exit one on a machine with no GPU too**, which is the whole reason the
/// capability is read off a constant: a refusal that had to build a backend
/// first would report a missing adapter here, and this test would have to
/// accept that and prove nothing.
#[test]
fn auxiliary_passes_are_refused_by_a_renderer_that_writes_none() {
    let model = repo_root().join("res/models/armadillo.obj");
    let Some(out) = render(&[
        model.to_str().expect("utf8 path"),
        "--out",
        "/dev/null.exr",
        "--engine",
        "raster",
        "--aov",
        "albedo",
    ]) else {
        return;
    };
    assert_eq!(
        code(&out),
        1,
        "expected a usage error; stderr said: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "the refusal wrote to standard output"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--aov"),
        "the message did not name the flag that could not take effect"
    );
}

/// A space for a file format that cannot carry one is the same kind of mistake.
#[test]
fn a_float_space_is_refused_where_the_output_is_not_a_float_file() {
    let model = repo_root().join("res/models/armadillo.obj");
    let Some(out) = render(&[
        model.to_str().expect("utf8 path"),
        "--out",
        "/dev/null.png",
        "--exr-space",
        "display",
    ]) else {
        return;
    };
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--exr-space"),
        "the message did not name the flag"
    );
}

/// Progress goes to standard error, and standard output stays empty.
///
/// The assertion holds whether or not this machine has a GPU, which is what
/// lets it run everywhere: the render either succeeds or exits on the missing
/// adapter, and the discipline under test is the same either way. Only the
/// success arm can check that anything was reported at all, so it does.
#[test]
fn progress_goes_to_standard_error_and_the_payload_stream_stays_empty() {
    let dir = std::env::temp_dir().join("solarxy-render-contract");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let out = dir.join("progress.png");
    let _ = std::fs::remove_file(&out);
    let model = repo_root().join("res/models/armadillo.obj");

    let Some(result) = render(&[
        model.to_str().expect("utf8 path"),
        "--out",
        out.to_str().expect("utf8 path"),
        "--res",
        "64x48",
    ]) else {
        return;
    };
    assert!(
        result.stdout.is_empty(),
        "the render put bytes on standard output: {:?}",
        String::from_utf8_lossy(&result.stdout)
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    match code(&result) {
        0 => {
            assert!(out.exists(), "a successful render wrote no file");
            assert!(
                stderr.contains("loading") && stderr.contains("done in"),
                "the progress sink reported nothing recognisable: {stderr}"
            );
            let _ = std::fs::remove_file(&out);
        }
        // No adapter. Every other code is a real failure of this test.
        4 => eprintln!("skipping the success arm: no GPU adapter"),
        other => panic!("unexpected exit {other}; stderr said: {stderr}"),
    }
}

/// The dashboard hands back to the plain line when it cannot have a terminal,
/// and standard output stays data either way.
///
/// A test runs with its streams piped, which is exactly the case the fallback
/// exists for, so this asserts the fallback rather than the surface. The
/// dashboard's own drawing is asserted in the crate, against a buffer, because
/// a test harness has no terminal to give it.
///
/// `--json` alongside it on purpose: the two composing is the whole reason the
/// dashboard paints on standard error, and a dashboard that ever wrote to
/// standard output would corrupt the payload this parses.
#[test]
fn the_dashboard_hands_back_to_the_plain_line_when_it_cannot_have_a_terminal() {
    let dir = std::env::temp_dir().join("solarxy-render-contract");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let out = dir.join("dashboard.png");
    let _ = std::fs::remove_file(&out);
    let model = repo_root().join("res/models/armadillo.obj");

    let Some(result) = render(&[
        model.to_str().expect("utf8 path"),
        "--out",
        out.to_str().expect("utf8 path"),
        "--res",
        "64x48",
        "--tui",
        "--json",
    ]) else {
        return;
    };
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("needs a terminal on standard error"),
        "the dashboard did not say why it stood down: {stderr}"
    );
    match code(&result) {
        0 => {
            assert!(
                stderr.contains("done in"),
                "the plain line did not take over: {stderr}"
            );
            // Checked by shape rather than by parsing it: this crate has no
            // JSON reader and is not worth one for an assertion that a
            // dashboard did not write here. A single escape sequence anywhere
            // in the stream breaks every one of these.
            let payload = String::from_utf8(result.stdout).expect("the report is text");
            assert!(
                payload.starts_with('{') && payload.trim_end().ends_with('}'),
                "standard output is not the report alone: {payload:?}"
            );
            assert!(
                payload.contains("schemaVersion"),
                "the report carries no schema version: {payload}"
            );
            assert!(
                !payload.contains('\u{1b}'),
                "an escape sequence reached the payload stream: {payload:?}"
            );
            let _ = std::fs::remove_file(&out);
        }
        4 => eprintln!("skipping the success arm: no GPU adapter"),
        other => panic!("unexpected exit {other}; stderr said: {stderr}"),
    }
}

/// A build without the window explains itself and renders anyway.
///
/// The acceptance criterion is that the flag still parses: a reader whose
/// build has no window should be told that, not told that `--watch` is not a
/// thing. Only meaningful in a build that does not have it, which is the
/// default set, and continuous integration builds with every feature so this
/// is compiled out exactly where it would be a lie.
#[cfg(not(feature = "watch"))]
#[test]
fn a_build_without_the_window_says_so_rather_than_refusing_the_flag() {
    let dir = std::env::temp_dir().join("solarxy-render-contract");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let out = dir.join("nowatch.png");
    let _ = std::fs::remove_file(&out);
    let model = repo_root().join("res/models/armadillo.obj");

    let Some(result) = render(&[
        model.to_str().expect("utf8 path"),
        "--out",
        out.to_str().expect("utf8 path"),
        "--res",
        "64x48",
        "--watch",
    ]) else {
        return;
    };
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("'watch' feature"),
        "the build did not name what it is missing: {stderr}"
    );
    assert!(
        !stderr.contains("unexpected argument"),
        "the flag was refused rather than explained: {stderr}"
    );
    match code(&result) {
        // It renders regardless: a missing window is not a missing render.
        0 => {
            assert!(out.exists(), "the render stopped because it had no window");
            let _ = std::fs::remove_file(&out);
        }
        4 => eprintln!("skipping the success arm: no GPU adapter"),
        other => panic!("unexpected exit {other}; stderr said: {stderr}"),
    }
}

/// A scene file whose bytes are not a scene exits two, saying so on stderr.
///
/// Two rather than three: the file is named correctly and is the right kind,
/// and what failed is reading it. Three is for a scene that read fine and then
/// could not be rendered. A build system retries the two differently, so the
/// distinction is the point.
#[test]
fn a_corrupt_scene_file_exits_two_with_nothing_on_standard_output() {
    let dir = std::env::temp_dir().join("solarxy-render-contract");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let corrupt = dir.join("corrupt.slxy");
    // A scene file is a zip archive. Bytes that are not one fail at the
    // container before anything reads a document out of it.
    std::fs::write(&corrupt, b"PK\x03\x04 and then nothing that follows").expect("the file writes");

    let Some(out) = render(&[
        corrupt.to_str().expect("utf8 path"),
        "--out",
        "/dev/null.png",
    ]) else {
        return;
    };
    assert_eq!(
        code(&out),
        2,
        "stderr said: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "a failure wrote to standard output");
    assert!(
        !out.stderr.is_empty(),
        "a failure said nothing about why it failed"
    );
    let _ = std::fs::remove_file(&corrupt);
}

/// Naming a render node the scene does not have exits three.
///
/// Three is the scene's code: the invocation was well formed and the file
/// loaded, and what failed was the document's own content. It is decided
/// before any device exists, which is what lets this run on a machine with no
/// GPU and still mean something.
///
/// The other two ways into three, a cook that fails and a scene carrying more
/// than one render node, need a scene authored to be broken and a scene
/// authored to be ambiguous. Neither exists as a fixture, and the code they
/// return is this one, so this is what stands for the class.
#[test]
fn naming_a_render_node_the_scene_lacks_exits_three() {
    let scene = repo_root().join("web/public/samples/lights-camera-review.slxy");
    if !scene.exists() {
        eprintln!("skipping: the sample scenes have not been generated");
        return;
    }
    let Some(out) = render(&[
        scene.to_str().expect("utf8 path"),
        "--out",
        "/dev/null.png",
        "--render-node",
        "no-node-is-called-this",
    ]) else {
        return;
    };
    assert_eq!(
        code(&out),
        3,
        "stderr said: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "a failure wrote to standard output");
}

/// An output that cannot be written exits seven, after the render.
///
/// Seven is the writing code, and it necessarily arrives late: the path is not
/// opened until there are bytes for it, so the render has to succeed first.
/// That is also why this skips on a machine with no adapter, which fails
/// earlier with four and never reaches the write.
#[test]
fn an_output_that_cannot_be_written_exits_seven() {
    let model = repo_root().join("res/models/armadillo.obj");
    // A directory that is not there. `write` does not create parents, so this
    // is a plain not-found at the moment of writing rather than a permission
    // trick that behaves differently as root.
    let missing = std::env::temp_dir()
        .join("solarxy-render-contract")
        .join("no-such-directory")
        .join("out.png");
    let Some(out) = render(&[
        model.to_str().expect("utf8 path"),
        "--out",
        missing.to_str().expect("utf8 path"),
        "--res",
        "64x48",
    ]) else {
        return;
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    match code(&out) {
        7 => assert!(out.stdout.is_empty(), "a failure wrote to standard output"),
        4 => eprintln!("skipping: no GPU adapter, so the write is never reached"),
        other => panic!("unexpected exit {other}; stderr said: {stderr}"),
    }
}

/// An interrupt exits six and leaves no half-written file behind.
///
/// Unix only, because the signal is. Rather than sleeping a guessed interval,
/// this reads the child's progress until sampling is announced, which is the
/// only window in which the interrupt tests anything: before it the cancel
/// flag has nothing to stop, and after it the file is already written.
///
/// Traced deliberately. A piped run writes one line per step and the step is
/// the tile, so the rasterizer's single non-progressive pass announces itself
/// and is over, leaving no window at all. The tracer announces the tile and
/// then keeps sampling it, which is the window.
///
/// `kill` rather than a signalling crate: one process spawn against a utility
/// every Unix has, versus a dependency in the tree of a public repository.
#[cfg(unix)]
#[test]
fn an_interrupt_exits_six_and_leaves_no_partial_file() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let bin = binary();
    if !bin.exists() {
        eprintln!("skipping: {} has not been built", bin.display());
        return;
    }
    let dir = std::env::temp_dir().join("solarxy-render-contract");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let out = dir.join("interrupted.png");
    let _ = std::fs::remove_file(&out);
    let model = repo_root().join("res/models/armadillo.obj");

    // Small and deep rather than large and shallow: the samples are what the
    // signal has to land inside, and the pixels are only cost.
    let mut child = Command::new(bin)
        .current_dir(repo_root())
        .args([
            "render",
            model.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--engine",
            "path-traced",
            "--res",
            "128x128",
            "--spp",
            "4096",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");

    let stderr = child.stderr.take().expect("stderr was piped");
    let mut reader = BufReader::new(stderr);
    let mut seen = String::new();
    let mut sampling = false;
    // The plain sink rewrites one line with carriage returns on a terminal and
    // writes a line per step otherwise, which is what a piped run gets.
    let mut line = Vec::new();
    while reader
        .read_until(b'\n', &mut line)
        .expect("the child's progress reads")
        > 0
    {
        seen.push_str(&String::from_utf8_lossy(&line));
        line.clear();
        if seen.contains("tile 1 of") {
            sampling = true;
            break;
        }
        if seen.contains("failed while") {
            break;
        }
    }

    if !sampling {
        let _ = child.kill();
        let _ = child.wait();
        // A machine with no adapter is the one reason to let this go. Anything
        // else, a flag this no longer accepts above all, would otherwise make
        // the test pass by never running, which is how it first went green
        // against an argument the command does not have.
        assert!(
            seen.contains("failed while loading"),
            "the render never reached drawing, and not for want of an adapter: {seen}"
        );
        eprintln!("skipping: no GPU adapter");
        return;
    }

    let killed = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(killed.success(), "the signal was not delivered");

    let status = child.wait().expect("the child exits");
    assert_eq!(
        status.code().unwrap_or(-1),
        6,
        "an interrupted render did not exit six"
    );
    assert!(
        !out.exists(),
        "an interrupted render left a file at {}",
        out.display()
    );
}

/// The subcommand did not displace the shipped surface.
///
/// The analyze and view modes are flags with users, and a pipeline that runs
/// them must keep running after this release.
#[test]
fn the_existing_modes_still_parse() {
    let bin = binary();
    if !bin.exists() {
        return;
    }
    let out = Command::new(bin)
        .arg("--help")
        .output()
        .expect("the binary runs");
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("--mode"), "the mode flag left the help");
    assert!(
        help.contains("render"),
        "the render command is not in the help"
    );
}

/// The terminal theme flags belong to the reader's terminal, not to one
/// surface, so they parse on either side of the render subcommand and are
/// listed in the help the README calls authoritative for it.
#[test]
fn the_theme_flags_work_after_the_render_subcommand() {
    let bin = binary();
    if !bin.exists() {
        return;
    }

    // The listing after the subcommand prints the same listing as before it,
    // and exits without rendering anything.
    let after = Command::new(&bin)
        .args(["render", "--list-tui-themes"])
        .output()
        .expect("the binary runs");
    let before = Command::new(&bin)
        .arg("--list-tui-themes")
        .output()
        .expect("the binary runs");
    assert_eq!(code(&after), 0, "the listing after the subcommand failed");
    assert_eq!(
        after.stdout, before.stdout,
        "the two listing forms print different listings"
    );
    assert!(
        !after.stdout.is_empty(),
        "the listing printed nothing at all"
    );

    // Both helps list both flags.
    for help_args in [&["--help"][..], &["render", "--help"][..]] {
        let out = Command::new(&bin)
            .args(help_args)
            .output()
            .expect("the binary runs");
        let help = String::from_utf8_lossy(&out.stdout);
        for flag in ["--tui-theme", "--list-tui-themes"] {
            assert!(help.contains(flag), "{flag} missing from {help_args:?}");
        }
    }

    // A theme named after the subcommand parses: the run gets far enough to
    // find the input missing (exit 2), where a rejected flag would have been
    // a usage error before anything was read.
    for theme_position in [
        &[
            "--tui-theme",
            "solarxy",
            "render",
            "no-such-file.obj",
            "-o",
            "out.png",
        ][..],
        &[
            "render",
            "no-such-file.obj",
            "-o",
            "out.png",
            "--tui-theme",
            "solarxy",
        ][..],
    ] {
        let out = Command::new(&bin)
            .args(theme_position)
            .current_dir(repo_root())
            .output()
            .expect("the binary runs");
        assert_eq!(
            code(&out),
            2,
            "the flag order {theme_position:?} did not reach the input load"
        );
    }
}
