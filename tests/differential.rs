//! Differential proof: the generated acceptance parser agrees with the
//! parsanol walker on every case, for every POC-subset grammar.
//!
//! Method: generate the Rust module, compile it as a standalone binary
//! with `rustc` (the same way proc-macro output is ultimately compiled),
//! feed inputs through it, and compare (accepted, end offset) with the
//! walker on the same grammar. A codegen/backend disagreement is the
//! bug class this crate exists to catch at birth.

use parsanol::portable::grammar::{Atom, Grammar};
use parsanol::portable::{to_parslet_compatible, AstArena, PortableParser};

use parsanol_codegen::generate_acceptance;

fn str_atom(pattern: &str, grammar: &mut Grammar) -> usize {
    grammar.add_atom(Atom::Str {
        pattern: pattern.to_string(),
    })
}

fn seq(grammar: &mut Grammar, atoms: Vec<usize>) -> usize {
    grammar.add_atom(Atom::Sequence { atoms })
}

fn alt(grammar: &mut Grammar, atoms: Vec<usize>) -> usize {
    grammar.add_atom(Atom::Alternative { atoms })
}

fn rep(grammar: &mut Grammar, atom: usize, min: usize, max: Option<usize>) -> usize {
    grammar.add_atom(Atom::Repetition {
        atom,
        min,
        max,
        tag: parsanol::portable::grammar::RepetitionTag::Repetition,
    })
}

fn re_atom(grammar: &mut Grammar, pattern: &str) -> usize {
    grammar.add_atom(Atom::Re {
        pattern: pattern.to_string(),
    })
}

fn named(grammar: &mut Grammar, name: &str, atom: usize) -> usize {
    grammar.add_atom(Atom::Named {
        name: name.to_string(),
        atom,
    })
}

/// The POC-subset corpus: eight grammars exercising sequences, ordered
/// choice with shared prefixes, greedy repetitions, optionals via `0*1`,
/// nesting, and transparent wrappers.
fn subset_grammars() -> Vec<(&'static str, Grammar, Vec<&'static str>)> {
    let mut cases = Vec::new();

    {
        let mut g = Grammar::new();
        let ab = str_atom("ab", &mut g);
        let cd = str_atom("cd", &mut g);
        let root = seq(&mut g, vec![ab, cd]);
        g.root = root;
        cases.push(("seq", g, vec!["abcd", "abc", "ab", "", "abcdx"]));
    }

    {
        let mut g = Grammar::new();
        let iso = str_atom("iso", &mut g);
        let ieee = str_atom("ieee", &mut g);
        let num = str_atom("12345", &mut g);
        let ident = alt(&mut g, vec![iso, ieee]);
        let space = str_atom(" ", &mut g);
        let root = seq(&mut g, vec![ident, space, num]);
        g.root = root;
        cases.push((
            "choice-shared-prefix",
            g,
            vec!["iso 12345", "ieee 12345", "ansi 12345", "iso  12345"],
        ));
    }

    {
        let mut g = Grammar::new();
        let a = str_atom("a", &mut g);
        let run = rep(&mut g, a, 1, None);
        let b = str_atom("b", &mut g);
        let root = seq(&mut g, vec![run, b]);
        g.root = root;
        cases.push(("greedy-rep", g, vec!["aaab", "b", "aaa", "aaacb"]));
    }

    {
        let mut g = Grammar::new();
        let open = str_atom("(", &mut g);
        let close = str_atom(")", &mut g);
        let word = str_atom("w", &mut g);
        let inner = rep(&mut g, word, 0, None);
        let group = seq(&mut g, vec![open, inner, close]);
        let groups = rep(&mut g, group, 1, None);
        let root = named(&mut g, "groups", groups);
        g.root = root;
        cases.push(("nested-groups", g, vec!["(w)(w)", "()", "(ww)", "(w)"]));
    }

    {
        let mut g = Grammar::new();
        let k = str_atom("k=", &mut g);
        let v = str_atom("v", &mut g);
        let vs = rep(&mut g, v, 1, None);
        let kv = seq(&mut g, vec![k, vs]);
        let semi = str_atom(";", &mut g);
        let seps = rep(&mut g, semi, 0, None);
        let root = seq(&mut g, vec![kv, seps]);
        g.root = root;
        cases.push(("optional-tail", g, vec!["k=v", "k=v;", "k=v;;;", "k="]));
    }

    {
        let mut g = Grammar::new();
        let hx = str_atom("0x", &mut g);
        let hx_upper = str_atom("0X", &mut g);
        let hex = alt(&mut g, vec![hx, hx_upper]);
        let seven = str_atom("7", &mut g);
        let digits = rep(&mut g, seven, 1, Some(4));
        let root = seq(&mut g, vec![hex, digits]);
        g.root = root;
        cases.push((
            "bounded-rep",
            g,
            vec!["0x77", "0X7777", "0x77777", "0x", "1x7"],
        ));
    }

    {
        let mut g = Grammar::new();
        let word = str_atom("w", &mut g);
        let inner = rep(&mut g, word, 0, None);
        let wrapped = named(&mut g, "inner", inner);
        let outer = named(&mut g, "outer", wrapped);
        let tail = rep(&mut g, word, 0, None);
        let root = seq(&mut g, vec![outer, tail]);
        g.root = root;
        cases.push(("transparent-wrappers", g, vec!["w", "www", ""]));
    }

    {
        let mut g = Grammar::new();
        let a = str_atom("a", &mut g);
        let b = str_atom("b", &mut g);
        let ab = alt(&mut g, vec![a, b]);
        let root = rep(&mut g, ab, 3, Some(3));
        g.root = root;
        cases.push(("exactly-three", g, vec!["abc", "abab", "aba", "ab"]));
    }

    {
        // hex=1F with class terminals: the walker's CharacterPattern fast
        // path, emitted as byte tests.
        let mut g = Grammar::new();
        let hex_ = str_atom("hex=", &mut g);
        let digit = re_atom(&mut g, "[0-9]");
        let hexdigit = re_atom(&mut g, "[0-9a-fA-F]");
        let value = seq(&mut g, vec![digit, hexdigit]);
        let root = seq(&mut g, vec![hex_, value]);
        g.root = root;
        cases.push((
            "class-hex",
            g,
            vec!["hex=1F", "hex=0a", "hex=A", "hex=G1", "hex=1"],
        ));
    }

    {
        // word runs over \w with a whitespace separator class.
        let mut g = Grammar::new();
        let word = re_atom(&mut g, r"\w");
        let run = rep(&mut g, word, 1, None);
        let ws = re_atom(&mut g, r"\s");
        let second = rep(&mut g, word, 1, None);
        let root = seq(&mut g, vec![run, ws, second]);
        g.root = root;
        cases.push((
            "class-word-ws",
            g,
            vec!["abc def", "a b", "ab  cd", "_1 x", " ", "abcdef"],
        ));
    }

    {
        // any-char runs (the quoted-string idiom) with the Any class.
        let mut g = Grammar::new();
        let quote = str_atom("\"", &mut g);
        let any = re_atom(&mut g, ".");
        let body = rep(&mut g, any, 0, None);
        let root = seq(&mut g, vec![quote, body, quote]);
        g.root = root;
        cases.push(("class-any", g, vec!["\"\"", "\"hi\"", "\"a\"b\"", "hi"]));
    }

    cases
}

fn walker_outcome(grammar: &Grammar, input: &str) -> Option<usize> {
    let mut arena = AstArena::for_input(input.len().max(64));
    let mut parser = PortableParser::new(grammar, input, &mut arena);
    parser.parse().ok()?;
    Some(input.len())
}

#[test]
fn generated_parsers_agree_with_the_walker() {
    for (name, grammar, inputs) in subset_grammars() {
        let source = generate_acceptance(&grammar)
            .unwrap_or_else(|err| panic!("{name}: generation failed: {err}"));

        let dir = tempfile::tempdir().expect("tempdir");
        let bin = build_case(
            dir.path(),
            &generated_binary_source(&source),
            name,
            parsanol_codegen::requires_regex(&grammar),
        );

        for input in inputs {
            let generated = run_case(&bin, input);
            let walker = walker_outcome(&grammar, input);
            assert_eq!(
                generated, walker,
                "{name}: divergence on {input:?} (generated {generated:?}, walker {walker:?})"
            );
        }
    }
}

/// Wrap the generated module in a line-oriented CLI: one input per line,
/// answers `OK <end>` or `FAIL`.
fn generated_binary_source(module: &str) -> String {
    // argv: inputs may contain newlines, so line-oriented stdin is out.
    format!(
        "{module}\n\
         fn main() {{\n\
         \x20   let input = std::env::args().nth(1).unwrap_or_default();\n\
         \x20   match parse(&input) {{\n\
         \x20       Some(end) => println!(\"OK {{end}}\"),\n\
         \x20       None => println!(\"FAIL\"),\n\
         \x20   }}\n\
         }}\n"
    )
}

fn run_case(bin: &std::path::Path, input: &str) -> Option<usize> {
    // argv, not stdin: inputs may contain newlines (trivia cases).
    let output = std::process::Command::new(bin)
        .arg(input)
        .output()
        .expect("run case");
    let text = String::from_utf8_lossy(&output.stdout);
    let answer = text.lines().next().unwrap_or("FAIL");
    if let Some(end) = answer.strip_prefix("OK ") {
        end.parse::<usize>().ok()
    } else {
        None
    }
}

/// Compile `source` as a line-oriented CLI binary. Regex-dependent modules
/// are built through a temporary cargo project (the generated code declares
/// `use regex::Regex;`), everything else goes through bare `rustc`.
fn build_case(
    dir: &std::path::Path,
    source: &str,
    name: &str,
    needs_regex: bool,
) -> std::path::PathBuf {
    if !needs_regex {
        let case = dir.join(format!("{name}.rs"));
        std::fs::write(&case, source).expect("write case");
        let bin = dir.join(name);
        let output = std::process::Command::new(rustc())
            .args([
                "--edition",
                "2021",
                "-O",
                "-o",
                bin.to_str().expect("path"),
                case.to_str().expect("path"),
            ])
            .output()
            .expect("rustc invocation");
        assert!(
            output.status.success(),
            "{name}: generated code did not compile:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return bin;
    }

    let proj = dir.join(format!("proj-{name}"));
    std::fs::create_dir_all(proj.join("src")).expect("create src");
    std::fs::write(
        proj.join("Cargo.toml"),
        "[package]\nname = \"case\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nregex = \"1\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::write(proj.join("src/main.rs"), source).expect("write main");
    let output = std::process::Command::new(cargo())
        .args(["build", "--release", "--quiet"])
        .current_dir(&proj)
        .output()
        .expect("cargo invocation");
    assert!(
        output.status.success(),
        "{name}: generated code did not compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    proj.join("target/release/case")
}

fn rustc() -> String {
    std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string())
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| {
        let home = std::env::var("CARGO_HOME")
            .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap_or_default()));
        format!("{home}/bin/cargo")
    })
}

// ---------------------------------------------------------------------------
// Shape differential (M1): the generated flattened value tree must equal
// the walker's parse_shape output — the flatten laws V0–V4, captures, and
// the merge rules included. Both sides render one canonical s-expression.
// ---------------------------------------------------------------------------

use parsanol::portable::ast::AstNode;

fn walker_shape(node: &AstNode, arena: &AstArena, input: &str) -> String {
    match node {
        AstNode::Nil => "(nil)".to_string(),
        AstNode::Bool(b) => format!("(text \"{b}\")"),
        AstNode::Int(i) => format!("(text \"{i}\")"),
        AstNode::Float(f) => format!("(text \"{f}\")"),
        AstNode::StringRef { pool_index } => {
            let text = arena.get_string(*pool_index as usize);
            format!("(text \"{}\")", escape(text))
        }
        AstNode::InputRef { offset, length } => {
            let text = &input[*offset as usize..(*offset + *length) as usize];
            format!("(text \"{}\")", escape(text))
        }
        AstNode::Array { pool_index, length } => {
            let mut out = String::from("(seq");
            for item in arena.get_array(*pool_index as usize, *length as usize) {
                out.push(' ');
                out.push_str(&walker_shape(&item, arena, input));
            }
            out.push(')');
            out
        }
        AstNode::Hash { pool_index, length } => {
            let mut pairs = arena.get_hash_items(*pool_index as usize, *length as usize);
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = String::from("(map");
            for (key, item) in pairs {
                out.push_str(&format!(
                    " (k \"{}\" {})",
                    escape(&key),
                    walker_shape(&item, arena, input)
                ));
            }
            out.push(')');
            out
        }
        other => format!("(other {:?})", other),
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

#[test]
fn generated_shape_parsers_agree_with_the_walker() {
    for (name, grammar, inputs) in subset_grammars() {
        let source = parsanol_codegen::generate_shape(&grammar)
            .unwrap_or_else(|err| panic!("{name}: shape generation failed: {err}"));

        let dir = tempfile::tempdir().expect("tempdir");
        let needs_regex = parsanol_codegen::requires_regex(&grammar);
        let bin = build_case(
            dir.path(),
            &generated_shape_binary_source(&source),
            "shape_case",
            needs_regex,
        );

        for input in inputs {
            let generated = run_shape_case(&bin, input);
            let walker = (|| {
                let mut arena = AstArena::for_input(input.len().max(64));
                let mut parser = PortableParser::new(&grammar, input, &mut arena);
                let ast = parser.parse().ok()?;
                let shaped = to_parslet_compatible(&ast, &mut arena, input);
                Some(walker_shape(&shaped, &arena, input))
            })();
            assert_eq!(
                generated, walker,
                "{name}: shape divergence on {input:?}\n  generated: {generated:?}\n  walker:    {walker:?}"
            );
        }
    }
}

/// Wrap the generated shape module in a line-oriented CLI: one input per
/// line, answers `OK <shape>` or `FAIL`.
fn generated_shape_binary_source(module: &str) -> String {
    // argv: inputs may contain newlines, so line-oriented stdin is out.
    format!(
        "{module}\n\
         fn main() {{\n\
         \x20   let input = std::env::args().nth(1).unwrap_or_default();\n\
         \x20   match parse_shape(&input) {{\n\
         \x20       Some(shape) => println!(\"OK {{shape}}\"),\n\
         \x20       None => println!(\"FAIL\"),\n\
         \x20   }}\n\
         }}\n"
    )
}

fn run_shape_case(bin: &std::path::Path, input: &str) -> Option<String> {
    // argv, not stdin: inputs may contain newlines (trivia cases).
    let output = std::process::Command::new(bin)
        .arg(input)
        .output()
        .expect("run shape case");
    let text = String::from_utf8_lossy(&output.stdout);
    let answer = text.lines().next().unwrap_or("FAIL");
    answer.strip_prefix("OK ").map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// M3: trivia injection, captured trivia, and C9-folded chains. The artifact
// carries the skip rewrite and the left-recursion fold as plain wire atoms —
// the generated code must agree with the walker on exactly those grammars,
// with no re-derivation of its own. (The first run of this test caught a
// real walker bug instead: parsanol-rs#296 — merged strings across trivia
// gaps decoded as the raw input slice; it needs parsanol >= 0.18.20.)
// ---------------------------------------------------------------------------

fn trivia(grammar: &mut Grammar, inner: usize) -> usize {
    grammar.add_atom(Atom::Trivia { atom: inner })
}

fn trivia_capture(
    grammar: &mut Grammar,
    inner: usize,
    rules: Vec<(String, String)>,
    whitespace: Option<String>,
) -> usize {
    grammar.add_atom(Atom::TriviaCapture {
        atom: inner,
        rules,
        whitespace,
    })
}

#[test]
fn generated_parsers_handle_trivia_captures_and_folded_chains() {
    let mut cases: Vec<(&'static str, Grammar, Vec<&'static str>)> = Vec::new();

    {
        // Injected skip trivia: `ws* "a" ws* "b"` with diagnostics-transparent ws.
        let mut g = Grammar::new();
        let ws_re = re_atom(&mut g, r"\s");
        let ws = rep(&mut g, ws_re, 0, None);
        let skip = trivia(&mut g, ws);
        let a = str_atom("a", &mut g);
        let b = str_atom("b", &mut g);
        let root = seq(&mut g, vec![skip, a, skip, b, skip]);
        g.root = root;
        cases.push((
            "trivia-injection",
            g,
            vec!["a b", "a  b", "ab", " a b ", "a\n\tb"],
        ));
    }

    {
        // Captured trivia: line comments and whitespace ride along; the value
        // tree must not change shape because of them.
        let mut g = Grammar::new();
        let line = str_atom("//", &mut g);
        let not_nl = re_atom(&mut g, r"[^\n]");
        let rest = rep(&mut g, not_nl, 0, None);
        let newline = str_atom("\n", &mut g);
        let comment = seq(&mut g, vec![line, rest, newline]);
        let ws_re = re_atom(&mut g, r"\s");
        let ws = rep(&mut g, ws_re, 1, None);
        let comment_or_ws = alt(&mut g, vec![comment, ws]);
        let skip = trivia_capture(
            &mut g,
            comment_or_ws,
            vec![("//".to_string(), "line_comment".to_string())],
            Some("whitespace".to_string()),
        );
        let a = str_atom("a", &mut g);
        let b = str_atom("b", &mut g);
        let root = seq(&mut g, vec![a, skip, b]);
        g.root = root;
        cases.push((
            "trivia-capture",
            g,
            vec!["a b", "a // note\n b", "a\n b", "ab", "a // only"],
        ));
    }

    {
        // The C9 fold of left-recursive `expr: expr "+" num | num` — plain
        // sequence/repetition atoms in the artifact, values must fold the same.
        let mut g = Grammar::new();
        let digit = re_atom(&mut g, r"\d");
        let digits = rep(&mut g, digit, 1, None);
        let num = named(&mut g, "num", digits);
        let plus = str_atom("+", &mut g);
        let plus_num = seq(&mut g, vec![plus, num]);
        let tail = rep(&mut g, plus_num, 0, None);
        let head_tail = seq(&mut g, vec![num, tail]);
        let root = named(&mut g, "expr", head_tail);
        g.root = root;
        cases.push((
            "c9-folded-chain",
            g,
            vec!["1+2+3", "1", "12+345", "1+", "+1", "1 + 2"],
        ));
    }

    for (name, grammar, inputs) in cases {
        let source = parsanol_codegen::generate_shape(&grammar)
            .unwrap_or_else(|err| panic!("{name}: generation failed: {err}"));
        let dir = tempfile::tempdir().expect("tempdir");
        let bin = build_case(
            dir.path(),
            &generated_shape_binary_source(&source),
            name,
            parsanol_codegen::requires_regex(&grammar),
        );
        for input in inputs {
            let generated = run_shape_case(&bin, input);
            let walker = (|| {
                let mut arena = AstArena::for_input(input.len().max(64));
                let mut parser = PortableParser::new(&grammar, input, &mut arena);
                let ast = parser.parse().ok()?;
                let shaped = to_parslet_compatible(&ast, &mut arena, input);
                Some(walker_shape(&shaped, &arena, input))
            })();
            assert_eq!(
                generated, walker,
                "{name}: shape divergence on {input:?}\n  generated: {generated:?}\n  walker:    {walker:?}"
            );
        }
    }
}
