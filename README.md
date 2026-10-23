# parsanol-codegen

**PARG → Rust source code generation** — the compiled-backend frontier.

The honest benchmark says interpretation has a ceiling: tree-sitter/pest
parse the same document ~60x faster than parsanol's interpreted walker.
This crate closes that gap the pest way — compile the grammar to plain
Rust. The grammar input is the checksummed `parsanol-tree/v2` artifact, so
the generated code inherits the artifact's guarantees.

**POC status (v0.1):** the *acceptance subset* works and is proven:
generate → `rustc` → run, and the generated parser agrees with the
parsanol walker on every case of the differential suite (ordered choice,
greedy repetition with the zero-progress guard, nesting, transparent
wrappers, bounded repetitions). Anything outside the subset returns
`Err(Unsupported)` — never wrong code.

```rust
let source = parsanol_codegen::generate_acceptance(&grammar)?;
// → a standalone module exposing `parse(input: &str) -> Option<usize>`
```

The full roadmap to the compiled backend (values and flatten laws, regex
terminals, the corpus-wide differential bar, build integration, and the
benchmarks that publish the closing gap) is
[docs/tickets/0001-roadmap.md](docs/tickets/0001-roadmap.md).
