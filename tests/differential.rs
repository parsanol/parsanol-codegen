//! Differential proof: the generated acceptance parser agrees with the
//! parsanol walker on every case, for every POC-subset grammar.
//!
//! Method: generate the Rust module, compile it as a standalone binary
//! with `rustc` (the same way proc-macro output is ultimately compiled),
//! feed inputs through it, and compare (accepted, end offset) with the
//! walker on the same grammar. A codegen/backend disagreement is the
//! bug class this crate exists to catch at birth.

use parsanol::portable::grammar::{Atom, Grammar};
use parsanol::portable::{AstArena, PortableParser};

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
        let case = dir.path().join("case.rs");
        std::fs::write(&case, generated_binary_source(&source)).expect("write case");

        let output = std::process::Command::new(rustc())
            .args([
                "--edition",
                "2021",
                "-O",
                "-o",
                dir.path().join("case").to_str().expect("path"),
                case.to_str().expect("path"),
            ])
            .output()
            .expect("rustc invocation");
        assert!(
            output.status.success(),
            "{name}: generated code did not compile:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        for input in inputs {
            let generated = run_case(dir.path(), input);
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

fn run_case(dir: &std::path::Path, input: &str) -> Option<usize> {
    use std::io::Write;
    let mut child = std::process::Command::new(dir.join("case"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn case binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .and_then(|_| child.stdin.as_mut().expect("stdin").write_all(b"\n"))
        .expect("write input");
    let output = child.wait_with_output().expect("run case");
    let text = String::from_utf8_lossy(&output.stdout);
    let answer = text.lines().next().unwrap_or("FAIL");
    if let Some(end) = answer.strip_prefix("OK ") {
        end.parse::<usize>().ok()
    } else {
        None
    }
}

fn rustc() -> String {
    std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string())
}
