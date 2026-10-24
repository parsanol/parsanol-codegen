//! The closing-gap bench (M4): the generated parser's number beside the
//! interpreted ones — packrat walker (native walk), bytecode VM, and the
//! codegen output — on one conformance grammar over a realistic input.
//!
//! The generated parser is compiled once in setup (`rustc`) and run as a
//! persistent child; each iteration is one stdio roundtrip. The pipe
//! roundtrip costs a few microseconds and is part of the measured number
//! — at the fixture's input size the parse dominates, but treat the
//! generated column as an upper bound.
//!
//! Runs only when the corpus is present (PARSANOL_CORPUS_DIR or the
//! parsanol-rs submodule); criterion prints nothing otherwise.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use parsanol::portable::backend::{BytecodeBackend, PackratBackend, ParsingBackend};
use parsanol::portable::grammar::Grammar;
use std::io::{BufRead, Write};
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

fn corpus_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PARSANOL_CORPUS_DIR") {
        return Some(PathBuf::from(dir));
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("parsanol-rs")
        .join("corpus")
        .join("conformance");
    dir.is_dir().then_some(dir)
}

/// One `key = value` match stretched to ~2 KB: the skip_trivia grammar
/// accepts a single `1*alpha "=" 1*digit` pair with trivia riding between
/// tokens. Nothing may trail the digits — the #195 injection exemption
/// puts no skip wrapper after an Re-repetition, so a trailing newline
/// would fail the full-input match.
fn bench_input(field: usize) -> String {
    format!("  {} = {}", "k".repeat(field), "7".repeat(field))
}

struct Fixture {
    grammar: Grammar,
    input: String,
    generated: std::sync::Mutex<GeneratedProcess>,
}

struct GeneratedProcess {
    child: std::process::Child,
}

impl GeneratedProcess {
    fn accept(&mut self, input: &str) -> bool {
        let stdin = self.child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(input.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .expect("write input");
        let mut stdout = self.child.stdout.take().expect("stdout");
        let mut line = String::new();
        std::io::BufReader::new(&mut stdout)
            .read_line(&mut line)
            .expect("read answer");
        self.child.stdout = Some(stdout);
        line.starts_with("OK ")
    }
}

fn fixture() -> Option<&'static Fixture> {
    static FIXTURE: OnceLock<Option<Fixture>> = OnceLock::new();
    let slot = FIXTURE.get_or_init(|| {
        let dir = corpus_dir()?;
        let artifact_path = dir.join("skip_trivia.json");
        let envelope: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(artifact_path).ok()?).ok()?;
        let default = envelope["default_entry"].as_str()?.to_string();
        let grammar: Grammar =
            serde_json::from_value(envelope["entries"][&default]["grammar"].clone()).ok()?;

        let source = parsanol_codegen::generate_acceptance(&grammar).ok()?;
        let dir = tempfile::tempdir().ok()?;
        let bin = compile_generated(
            dir.path(),
            &source,
            parsanol_codegen::requires_regex(&grammar),
        )?;
        let child = std::process::Command::new(&bin)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        // Keep the tempdir alive for the bench process lifetime.
        std::mem::forget(dir);
        Some(Fixture {
            grammar,
            input: bench_input(1024),
            generated: std::sync::Mutex::new(GeneratedProcess { child }),
        })
    });
    slot.as_ref()
}

/// Compile the generated module: bare `rustc` for regex-free modules,
/// a temporary cargo project (with the regex dependency) otherwise.
fn compile_generated(dir: &Path, module: &str, needs_regex: bool) -> Option<PathBuf> {
    let wrapped = wrap_acceptance_cli(module);
    let bin = dir.join("generated");
    if !needs_regex {
        let case = dir.join("generated.rs");
        std::fs::write(&case, wrapped).ok()?;
        let output =
            std::process::Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
                .args(["--edition", "2021", "-O", "-o"])
                .arg(&bin)
                .arg(&case)
                .output()
                .ok()?;
        if !output.status.success() {
            eprintln!(
                "closing_gap: generated module did not compile:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return None;
        }
        return Some(bin);
    }

    let proj = dir.join("proj");
    std::fs::create_dir_all(proj.join("src")).ok()?;
    std::fs::write(
        proj.join("Cargo.toml"),
        "[package]\nname = \"generated\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nregex = \"1\"\n\n[[bin]]\nname = \"generated\"\npath = \"src/main.rs\"\n",
    )
    .ok()?;
    std::fs::write(proj.join("src/main.rs"), wrapped).ok()?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| {
        let home = std::env::var("CARGO_HOME")
            .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap_or_default()));
        format!("{home}/bin/cargo")
    });
    let output = std::process::Command::new(cargo)
        .args(["build", "--release", "--quiet"])
        .current_dir(&proj)
        .output()
        .ok()?;
    if !output.status.success() {
        eprintln!(
            "closing_gap: generated module did not compile:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }
    Some(proj.join("target/release/generated"))
}

fn wrap_acceptance_cli(module: &str) -> String {
    format!(
        "{module}\n\
         use std::io::{{BufRead, Write}};\n\n\
         fn main() {{\n\
         \x20   let stdin = std::io::stdin();\n\
         \x20   let mut out = std::io::stdout();\n\
         \x20   for line in stdin.lock().lines() {{\n\
         \x20       let line = line.expect(\"stdin\");\n\
         \x20       match parse(&line) {{\n\
         \x20           Some(end) => writeln!(out, \"OK {{end}}\").unwrap(),\n\
         \x20           None => writeln!(out, \"FAIL\").unwrap(),\n\
         \x20       }}\n\
         \x20   }}\n\
         }}\n"
    )
}

fn closing_gap(c: &mut Criterion) {
    let Some(fixture) = fixture() else {
        eprintln!("closing_gap: corpus not present — skipping");
        return;
    };
    let mut group = c.benchmark_group("closing-gap/skip_trivia");
    group.throughput(Throughput::Bytes(fixture.input.len() as u64));

    let mut generated = fixture.generated.lock().expect("generated child");
    group.bench_function("generated", |b| {
        b.iter(|| {
            assert!(generated.accept(&fixture.input));
        })
    });
    drop(generated);

    group.bench_with_input(
        BenchmarkId::new("walker", "packrat"),
        &fixture.input,
        |b, input| {
            b.iter(|| {
                let mut backend = PackratBackend::new();
                assert!(backend.parse(&fixture.grammar, input).is_ok());
            })
        },
    );

    group.bench_with_input(
        BenchmarkId::new("vm", "bytecode"),
        &fixture.input,
        |b, input| {
            b.iter(|| {
                let mut backend = BytecodeBackend::new();
                assert!(backend.parse(&fixture.grammar, input).is_ok());
            })
        },
    );

    group.finish();
}

criterion_group!(benches, closing_gap);
criterion_main!(benches);
