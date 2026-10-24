//! parg-codegen — compile a `parsanol-tree/v2` artifact to a standalone
//! Rust parser module.
//!
//!     parg-codegen <artifact.json> [--entry NAME] [--mode shape|acceptance] [--out PATH]
//!
//! The envelope checksum is verified before generation (artifact in, Rust
//! out). With `--out` the module is written to a file (the build.rs flow);
//! otherwise it goes to stdout.

use std::path::PathBuf;
use std::process::ExitCode;

enum Mode {
    Acceptance,
    Shape,
}

fn fail(message: &str) -> ExitCode {
    eprintln!("parg-codegen: {message}");
    eprintln!(
        "usage: parg-codegen <artifact.json> [--entry NAME] [--mode acceptance|shape] [--out PATH]"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut artifact: Option<PathBuf> = None;
    let mut entry: Option<String> = None;
    let mut mode = Mode::Shape;
    let mut out: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--entry" => match args.next() {
                Some(name) => entry = Some(name),
                None => return fail("--entry needs a value"),
            },
            "--mode" => match args.next().as_deref() {
                Some("acceptance") => mode = Mode::Acceptance,
                Some("shape") => mode = Mode::Shape,
                _ => return fail("--mode is acceptance or shape"),
            },
            "--out" => match args.next() {
                Some(path) => out = Some(PathBuf::from(path)),
                None => return fail("--out needs a value"),
            },
            other if other.starts_with('-') => return fail(&format!("unknown flag {other}")),
            other => {
                if artifact.is_some() {
                    return fail("exactly one artifact path expected");
                }
                artifact = Some(PathBuf::from(other));
            }
        }
    }

    let Some(path) = artifact else {
        return fail("an artifact path is required");
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => return fail(&format!("{}: {err}", path.display())),
    };

    let mode = match mode {
        Mode::Acceptance => parsanol_codegen::Mode::Acceptance,
        Mode::Shape => parsanol_codegen::Mode::Shape,
    };
    match parsanol_codegen::generate_artifact(&text, entry.as_deref(), mode) {
        Ok(source) => match out {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::write(&path, source) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(err) => fail(&format!("{}: {err}", path.display())),
                }
            }
            None => {
                print!("{source}");
                ExitCode::SUCCESS
            }
        },
        Err(err) => fail(&format!("{}: {err}", path.display())),
    }
}
