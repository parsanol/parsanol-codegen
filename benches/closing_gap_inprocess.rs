//! The in-process closing-gap bench (M4/D2): the generated parser
//! linked into THIS process through its C-ABI entry (dlopen, one
//! setup cost, zero pipe overhead) beside the packrat walker and the
//! bytecode VM, on the same conformance grammar and input. This is
//! the library delivery shape; the sibling `closing_gap` bench
//! measures the CLI delivery shape (a persistent child, stdio
//! roundtrips in the number).
//!
//! The fixture is `recovery_kv` — literal/class atoms only, so the
//! generated module compiles standalone (no regex crate).

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use parsanol::portable::backend::{BytecodeBackend, PackratBackend, ParsingBackend};
use parsanol::portable::grammar::Grammar;
use std::path::{Path, PathBuf};
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

struct Loaded {
    parse: unsafe extern "C" fn(*const u8, usize) -> usize,
    _lib: libloading::Library,
}

unsafe fn try_load_generated(source: &str, dir: &Path) -> Option<Loaded> {
    let rs = dir.join("generated_parse.rs");
    let so = dir.join("libgenerated_parse.so");
    std::fs::write(&rs, source).expect("write generated module");
    let status = std::process::Command::new("rustc")
        .args([
            "--edition=2021",
            "--crate-type=cdylib",
            "--crate-name=generated_parse",
            "-O",
            "-o",
        ])
        .arg(&so)
        .arg(&rs)
        .status()
        .ok()?
        .success();
    if !status {
        return None;
    }
    let lib = libloading::Library::new(&so).ok()?;
    let parse = *lib
        .get::<unsafe extern "C" fn(*const u8, usize) -> usize>(b"parsanol_generated_parse")
        .ok()?;
    Some(Loaded { parse, _lib: lib })
}

fn fixture() -> &'static (Grammar, String, Loaded) {
    static FIXTURE: OnceLock<(Grammar, String, Loaded)> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let dir = corpus_dir().expect("the conformance corpus (PARSANOL_CORPUS_DIR or parsanol-rs/)");
        // The first artifact whose generated module is self-contained
        // (no regex crate): the bench compiles it standalone, so the
        // number carries no rustc-version coupling to this build.
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .expect("corpus dir")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        let (mut rustc_fail, mut rejected) = (0usize, 0usize);
        let mut best: Option<(Grammar, String, Loaded)> = None;
        for artifact_path in paths {
            let Ok(text) = std::fs::read_to_string(&artifact_path) else {
                continue;
            };
            let Ok(artifact) = parsanol::parg::PargArtifact::from_json(&text) else {
                continue;
            };
            let Ok(raw) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let Some(entry) = artifact.default_entry().map(str::to_string) else {
                continue;
            };
            let Ok(grammar) = artifact.grammar(&entry) else {
                continue;
            };
            if parsanol_codegen::requires_regex(&grammar) {
                continue;
            }
            let Ok(source) = parsanol_codegen::generate(&grammar, parsanol_codegen::Mode::Acceptance)
            else {
                continue;
            };
            // The fixture input is the artifact's own accept case,
            // repeated — a document that tolerates its own
            // repetition (rep-spine grammars) scales to a size where
            // the parse dominates. Artifacts whose language does
            // not, are skipped.
            let Some(accept) = raw["tests"]
                .as_array()
                .and_then(|tests| {
                    tests
                        .iter()
                        .find(|test| test["kind"] == "accept")
                        .and_then(|test| test["input"].as_str())
                })
                .map(str::to_string)
            else {
                continue;
            };
            let loaded = unsafe { try_load_generated(&source, &std::env::temp_dir()) };
            let Some(loaded) = loaded else {
                rustc_fail += 1;
                continue;
            };
            // The largest repetition of the artifact's own accept case
            // the generated parser takes: grammars bound their
            // documents, so the count descends to a floor of one.
            // The largest repetition of the artifact's own accept case
            // the generated parser takes; the best across candidates
            // wins (the parse should dominate the measurement).
            let input = [256usize, 128, 64, 32, 16, 8, 4, 2, 1]
                .into_iter()
                .map(|count| accept.repeat(count))
                .find(|candidate| unsafe {
                    (loaded.parse)(candidate.as_ptr(), candidate.len()) > 0
                });
            let Some(input) = input else {
                rejected += 1;
                eprintln!("candidate rejected at every scale: {}", artifact_path.display());
                continue;
            };
            let better = best
                .as_ref()
                .is_none_or(|(_, best_input, _)| input.len() > best_input.len());
            if better {
                eprintln!(
                    "candidate: {} (input {} bytes)",
                    artifact_path.display(),
                    input.len()
                );
                best = Some((grammar, input, loaded));
            }
        }
        let Some((grammar, input, loaded)) = best else {
            panic!(
                "no self-contained corpus artifact for the bench (tried {}: rustc_fail {}, rejected {})",
                rustc_fail + rejected,
                rustc_fail,
                rejected
            );
        };
        eprintln!("fixture: {} bytes", input.len());
        (grammar, input, loaded)
    })
}

fn bench_closing_gap_inprocess(c: &mut Criterion) {
    if corpus_dir().is_none() {
        eprintln!("skipping: the conformance corpus is not present");
        return;
    }
    let fixture = fixture();
    let (grammar, input, loaded) = (&fixture.0, &fixture.1, &fixture.2);
    let mut group = c.benchmark_group("closing_gap_inprocess");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.bench_function(BenchmarkId::new("generated", input.len()), |b| {
        b.iter(|| unsafe {
            let n = (loaded.parse)(input.as_ptr(), input.len());
            assert!(n > 0, "generated parser rejected the input");
        })
    });
    let mut packrat = PackratBackend::new();
    group.bench_function(BenchmarkId::new("walker_packrat", input.len()), |b| {
        b.iter(|| {
            let tree = packrat.parse(grammar, input).expect("walker parsed");
            let _ = tree;
        })
    });
    let mut vm = BytecodeBackend::new();
    group.bench_function(BenchmarkId::new("bytecode_vm", input.len()), |b| {
        b.iter(|| {
            let tree = vm.parse(grammar, input).expect("vm parsed");
            let _ = tree;
        })
    });
    group.finish();
}

criterion_group!(benches, bench_closing_gap_inprocess);
criterion_main!(benches);
