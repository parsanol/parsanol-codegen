//! The macro compiles the artifact at build time and the generated
//! parser parses — the proc-macro delivery shape proven end to end.

use parsanol_codegen_macros::parg_parser;

// The path resolves against this test crate's manifest (the workspace
// root); the corpus rides the parsanol-rs submodule.
parg_parser!("testdata/kv.artifact.json");

// Full-regex terminals import the regex crate: consumers declare it
// when parsanol_codegen::requires_regex is true for their grammar
// (the recovery_kv fixture has a gap-class terminal).
#[test]
fn the_generated_parser_accepts_the_grammar() {
    assert!(parse("a=1;b=2;").is_some());
    assert!(parse("a=1;b2;").is_none());
}
