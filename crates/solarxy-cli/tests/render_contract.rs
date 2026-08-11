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
//! The codes for a lost device and for an unavailable adapter. Both need a
//! machine whose GPU behaves in a particular way, and a test that provokes one
//! by breaking the machine is worse than the gap. They are exercised by hand and
//! recorded.

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
