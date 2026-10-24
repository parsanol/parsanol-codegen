//! M1 corpus-wide bar: every conformance artifact whose grammar fits the
//! POC subset is generated in shape mode and replayed against its embedded
//! accept / reject / shape tests. Artifacts using atoms outside the subset
//! are counted as skipped, never silently wrong.
//!
//! The bar rises as the subset grows: full-regex terminals already brought
//! most lexical grammars in; captures and the flatten laws are proven by
//! `tests/differential.rs`.

use parsanol::portable::grammar::Grammar;
use parsanol_codegen::generate_shape;
use serde_json::Value;
use std::path::{Path, PathBuf};

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

/// The generated `render` form for a JSON expectation — the comparison
/// form `parse_shape` emits (terminals reduce to their text).
fn canonical_render(value: &Value) -> String {
    match value {
        Value::Null => "(nil)".to_string(),
        Value::Bool(b) => format!("(text \"{b}\")"),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                format!("(text \"{int}\")")
            } else if let Some(float) = number.as_f64() {
                if float.fract() == 0.0 && float.is_finite() {
                    format!("(text \"{float:.1}\")")
                } else {
                    format!("(text \"{float}\")")
                }
            } else {
                "(nil)".to_string()
            }
        }
        Value::String(text) => format!("(text \"{}\")", escape(text)),
        Value::Array(items) => {
            let mut out = String::from("(seq");
            for item in items {
                out.push(' ');
                out.push_str(&canonical_render(item));
            }
            out.push(')');
            out
        }
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let mut out = String::from("(map");
            for key in keys {
                out.push_str(&format!(
                    " (k \"{}\" {})",
                    escape(key),
                    canonical_render(&object[key])
                ));
            }
            out.push(')');
            out
        }
    }
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

fn run_shape_case(bin: &Path, input: &str) -> Option<String> {
    // argv, not stdin: corpus inputs may contain newlines (CRLF cases).
    let output = std::process::Command::new(bin)
        .arg(input)
        .output()
        .expect("run shape case");
    let text = String::from_utf8_lossy(&output.stdout);
    let answer = text.lines().next().unwrap_or("FAIL");
    answer.strip_prefix("OK ").map(|s| s.to_string())
}

fn run_acceptance_case(bin: &Path, input: &str) -> bool {
    let output = std::process::Command::new(bin)
        .arg(input)
        .output()
        .expect("run acceptance case");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("FAIL")
        .starts_with("OK")
}

#[test]
fn corpus_subset_replays_through_generated_parsers() {
    let Some(dir) = corpus_dir() else {
        eprintln!("skipping: corpus not present (set PARSANOL_CORPUS_DIR)");
        return;
    };

    let mut generated = 0usize;
    let mut skipped = 0usize;
    let mut cases = 0usize;
    let divergences: Vec<String> = Vec::new();

    let mut artifacts: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("corpus dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    artifacts.sort();

    for artifact_path in &artifacts {
        let envelope: Value = serde_json::from_str(
            &std::fs::read_to_string(artifact_path).expect("artifact readable"),
        )
        .expect("artifact JSON");
        let entries = envelope
            .get("entries")
            .and_then(Value::as_object)
            .expect("entries object");
        let default_entry = envelope.get("default_entry").and_then(Value::as_str);

        // One generated binary per artifact: the default entry only (the
        // multi-entry artifacts' other entries ride the same grammar).
        let Some(entry_name) = default_entry.or_else(|| entries.keys().next().map(|k| k.as_str()))
        else {
            skipped += 1;
            continue;
        };
        let Some(grammar_value) = entries
            .get(entry_name)
            .and_then(|entry| entry.get("grammar"))
            .cloned()
        else {
            skipped += 1;
            continue;
        };
        let grammar: Grammar = match serde_json::from_value(grammar_value) {
            Ok(grammar) => grammar,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let source = match generate_shape(&grammar) {
            Ok(source) => source,
            Err(unsupported) => {
                skipped += 1;
                eprintln!("skip: {} — {unsupported}", artifact_path.display());
                continue;
            }
        };
        // Acceptance mode rides the same subset; every artifact replays
        // in both modes (the acceptance emitter had its own value-channel
        // bugs, found only once artifacts ran through it).
        let acceptance_source = parsanol_codegen::generate_acceptance(&grammar)
            .expect("acceptance generates wherever shape does");

        let dir = tempfile::tempdir().expect("tempdir");
        let needs_regex = parsanol_codegen::requires_regex(&grammar);
        let bins = crate_build_bins(
            dir.path(),
            &format!("artifact_{generated}"),
            &[
                (
                    "shape_case",
                    &format!(
                        "{source}\nfn main() {{\n    let input = std::env::args().nth(1).expect(\"input\");\n    match parse_shape(&input) {{\n        Some(shape) => println!(\"OK {{shape}}\"),\n        None => println!(\"FAIL\"),\n    }}\n}}\n"
                    ),
                ),
                (
                    "acceptance_case",
                    &format!(
                        "{acceptance_source}\nfn main() {{\n    let input = std::env::args().nth(1).expect(\"input\");\n    match parse(&input) {{\n        Some(_) => println!(\"OK\"),\n        None => println!(\"FAIL\"),\n    }}\n}}\n"
                    ),
                ),
            ],
            needs_regex,
        );
        let bin = &bins[0];
        let acceptance_bin = &bins[1];
        generated += 1;

        for test in envelope
            .get("tests")
            .and_then(Value::as_array)
            .expect("tests")
        {
            let test_entry = test.get("entry").and_then(Value::as_str);
            if test_entry.is_some() && test_entry != Some(entry_name) {
                continue;
            }
            let kind = test.get("kind").and_then(Value::as_str).unwrap_or("accept");
            if !matches!(kind, "accept" | "reject" | "shape") {
                continue; // capture/example need structured captures
            }
            let input = test.get("input").and_then(Value::as_str).expect("input");
            cases += 1;
            let shape = run_shape_case(bin, input);
            let accepted = run_acceptance_case(acceptance_bin, input);
            match kind {
                "accept" => {
                    assert!(
                        shape.is_some(),
                        "{}: expected acceptance for {input:?}",
                        artifact_path.display()
                    );
                    assert!(
                        accepted,
                        "{}: acceptance-mode parser rejected {input:?}",
                        artifact_path.display()
                    );
                }
                "reject" => {
                    assert!(
                        shape.is_none(),
                        "{}: expected rejection for {input:?}, got {shape:?}",
                        artifact_path.display()
                    );
                    assert!(
                        !accepted,
                        "{}: acceptance-mode parser accepted {input:?}",
                        artifact_path.display()
                    );
                }
                "shape" => {
                    // parse_shape already renders the canonical form.
                    let expect = test.get("expect").cloned().unwrap_or(Value::Null);
                    let rendered = shape;
                    let want = canonical_render(&expect);
                    assert_eq!(
                        rendered.as_deref(),
                        Some(want.as_str()),
                        "{}: shape divergence for {input:?}",
                        artifact_path.display()
                    );
                }
                _ => unreachable!(),
            }
        }
    }

    eprintln!(
        "corpus codegen: {generated} artifacts generated, {skipped} outside the POC subset, {cases} cases replayed"
    );
    eprintln!(
        "corpus codegen result: {generated} generated, {skipped} outside the subset, {cases} cases replayed"
    );
    assert!(generated > 0, "no corpus artifact fit the POC subset");
    assert!(
        divergences.is_empty(),
        "{} divergence(s):\n{}",
        divergences.len(),
        divergences.join("\n")
    );
}

fn crate_build(dir: &Path, source: &str, name: &str, needs_regex: bool) -> PathBuf {
    crate_build_bins(dir, name, &[("case", source)], needs_regex).remove(0)
}

/// One cargo project, several `[[bin]]` targets: acceptance and shape
/// parsers for the same artifact compile in a single build.
fn crate_build_bins(
    dir: &Path,
    name: &str,
    bins: &[(&str, &str)],
    needs_regex: bool,
) -> Vec<PathBuf> {
    let proj = dir.join(name);
    std::fs::create_dir_all(proj.join("src")).expect("create src");
    let deps = if needs_regex {
        "\n[dependencies]\nregex = \"1\"\n"
    } else {
        ""
    };
    let targets: String = bins
        .iter()
        .map(|(bin, _)| format!("[[bin]]\nname = \"{bin}\"\npath = \"src/{bin}.rs\"\n"))
        .collect();
    std::fs::write(
        proj.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{}\n{targets}",
            name.replace('-', "_"),
            deps
        ),
    )
    .expect("write Cargo.toml");
    for (bin, source) in bins {
        std::fs::write(proj.join(format!("src/{bin}.rs")), source).expect("write bin");
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| {
        let home = std::env::var("CARGO_HOME")
            .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap_or_default()));
        format!("{home}/bin/cargo")
    });
    let output = std::process::Command::new(cargo)
        .args(["build", "--release", "--quiet"])
        .current_dir(&proj)
        .output()
        .expect("cargo invocation");
    assert!(
        output.status.success(),
        "generated code did not compile:\n{}\n(source saved to /tmp/fail_{name}.rs)",
        String::from_utf8_lossy(&output.stderr)
    );
    bins.iter()
        .map(|(bin, _)| proj.join(format!("target/release/{bin}")))
        .collect()
}

#[test]
fn artifact_reader_generates_and_replays() {
    let Some(dir) = corpus_dir() else {
        eprintln!("skipping: corpus not present");
        return;
    };
    // The artifact path in, generated module out — the M4 seam.
    let path = dir.join("recovery_resync_declared.json");
    let envelope_text = std::fs::read_to_string(&path).expect("readable");
    let envelope: Value = serde_json::from_str(&envelope_text).expect("artifact JSON");
    let source =
        parsanol_codegen::generate_artifact(&envelope_text, None, parsanol_codegen::Mode::Shape)
            .expect("artifact generates");

    let dir = tempfile::tempdir().expect("tempdir");
    let default = envelope["default_entry"].as_str().expect("default entry");
    let grammar: Grammar = serde_json::from_value(envelope["entries"][default]["grammar"].clone())
        .expect("grammar parses");
    let needs_regex = parsanol_codegen::requires_regex(&grammar);
    let bin = crate_build(
        dir.path(),
        &format!(
            "{source}\nfn main() {{\n    let input = std::env::args().nth(1).expect(\"input\");\n    match parse_shape(&input) {{\n        Some(shape) => println!(\"OK {{shape}}\"),\n        None => println!(\"FAIL\"),\n    }}\n}}\n"
        ),
        "artifact_reader",
        needs_regex,
    );
    let input = envelope["tests"]
        .as_array()
        .and_then(|tests| {
            tests
                .iter()
                .find(|test| test.get("kind").and_then(Value::as_str) == Some("accept"))
        })
        .and_then(|test| test.get("input").and_then(Value::as_str))
        .expect("accept test present")
        .to_string();
    let output = std::process::Command::new(bin)
        .arg(&input)
        .output()
        .expect("run artifact-generated binary");
    let text = String::from_utf8_lossy(&output.stdout);
    let answer = text.lines().next().unwrap_or("");
    assert!(
        answer.starts_with("OK "),
        "artifact-generated binary rejected {input:?}: stdout={text:?} stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_generates_compilable_modules() {
    let Some(dir) = corpus_dir() else {
        eprintln!("skipping: corpus not present");
        return;
    };
    // The delivery pipeline end-to-end: the CLI binary reads the
    // artifact and emits the module, the module compiles standalone,
    // and the binary accepts the artifact's own test input.
    let artifact = dir.join("recovery_resync_declared.json");
    let envelope: Value =
        serde_json::from_str(&std::fs::read_to_string(&artifact).expect("artifact readable"))
            .expect("artifact JSON");

    let generated = tempfile::tempdir().expect("tempdir");
    let module = generated.path().join("parser.rs");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_parg-codegen"))
        .arg(&artifact)
        .arg("--mode")
        .arg("shape")
        .arg("--out")
        .arg(&module)
        .output()
        .expect("run parg-codegen");
    assert!(
        output.status.success(),
        "parg-codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let default = envelope["default_entry"].as_str().expect("default entry");
    let grammar: Grammar = serde_json::from_value(envelope["entries"][default]["grammar"].clone())
        .expect("grammar parses");
    let source = std::fs::read_to_string(&module).expect("generated module readable");
    let input = envelope["tests"]
        .as_array()
        .and_then(|tests| {
            tests
                .iter()
                .find(|test| test.get("kind").and_then(Value::as_str) == Some("accept"))
        })
        .and_then(|test| test.get("input").and_then(Value::as_str))
        .expect("accept test present");

    let build = tempfile::tempdir().expect("tempdir");
    let bin = crate_build(
        build.path(),
        &format!(
            "{source}\nfn main() {{\n    let input = std::env::args().nth(1).expect(\"input\");\n    match parse_shape(&input) {{\n        Some(shape) => println!(\"OK {{shape}}\"),\n        None => println!(\"FAIL\"),\n    }}\n}}\n"
        ),
        "cli_case",
        parsanol_codegen::requires_regex(&grammar),
    );
    let shape = run_shape_case(&bin, input);
    assert!(
        shape.is_some(),
        "CLI-generated parser rejected its own artifact's accept case {input:?}"
    );
}
