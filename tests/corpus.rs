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

#[test]
fn corpus_subset_replays_through_generated_parsers() {
    let Some(dir) = corpus_dir() else {
        eprintln!("skipping: corpus not present (set PARSANOL_CORPUS_DIR)");
        return;
    };

    let mut generated = 0usize;
    let mut skipped = 0usize;
    let mut cases = 0usize;
    let mut divergences: Vec<String> = Vec::new();

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

        let dir = tempfile::tempdir().expect("tempdir");
        let needs_regex = parsanol_codegen::requires_regex(&grammar);
        let bin = crate_build(dir.path(), &format!(
            "{}\nfn main() {{\n    let input = std::env::args().nth(1).expect(\"input\");\n    match parse_shape(&input) {{\n        Some(shape) => println!(\"OK {{shape}}\"),\n        None => println!(\"FAIL\"),\n    }}\n}}\n",
            source
        ), &format!("artifact_{generated}"), needs_regex);
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
            let shape = run_shape_case(&bin, input);
            match kind {
                "accept" => assert!(
                    shape.is_some(),
                    "{}: expected acceptance for {input:?}",
                    artifact_path.display()
                ),
                "reject" => assert!(
                    shape.is_none(),
                    "{}: expected rejection for {input:?}, got {shape:?}",
                    artifact_path.display()
                ),
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
    let proj = dir.join(name);
    std::fs::create_dir_all(proj.join("src")).expect("create src");
    let deps = if needs_regex {
        "\n[dependencies]\nregex = \"1\"\n"
    } else {
        ""
    };
    std::fs::write(
        proj.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{}\n[[bin]]\nname = \"case\"\npath = \"src/main.rs\"\n",
            name.replace('-', "_"),
            deps
        ),
    )
    .expect("write Cargo.toml");
    std::fs::write(proj.join("src/main.rs"), source).expect("write main");
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
        "generated code did not compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    proj.join("target/release/case")
}
