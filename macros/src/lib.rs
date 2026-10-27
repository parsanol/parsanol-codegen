//! `parg_parser!` — compile a PARG artifact into parser code at Rust
//! compile time (the proc-macro delivery shape of codegen#1 M4).
//!
//! ```ignore
//! use parsanol_codegen_macros::parg_parser;
//!
//! parg_parser!("artifacts/kv.artifact.json", entry = "main");
//! // generates: fn parse(input: &str) -> Option<usize>
//! ```
//!
//! The macro runs the same generator the CLI ships: the artifact is
//! checksum-verified at compile time, and the emitted module is the
//! byte-stable output of parsanol-codegen's generator. The artifact
//! path resolves relative to the invoking crate's manifest.

use proc_macro::TokenStream;

/// Compile the artifact at `path` (relative to the invoking crate's
/// manifest) and emit its generated parser module.
#[proc_macro]
pub fn parg_parser(input: TokenStream) -> TokenStream {
    let rendered = input.to_string();
    let path_literal = rendered
        .trim()
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("parg_parser! takes a string literal path");
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(&manifest).join(path_literal);
    let envelope = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("parg_parser!: cannot read {}: {error}", path.display()));
    let source =
        parsanol_codegen::generate_artifact(&envelope, None, parsanol_codegen::Mode::Acceptance)
            .unwrap_or_else(|error| panic!("parg_parser!: {error}"));
    source.parse().expect("the generated module is valid Rust")
}
