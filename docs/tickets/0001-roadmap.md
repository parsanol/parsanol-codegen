# 0001 — Roadmap: from acceptance POC to the compiled backend

The POC proves the approach: generated Rust parsers are tree/behavior-
identical to the walker on the acceptance subset (see
`tests/differential.rs` — generate, `rustc`, run, compare). The compiled
backend exists to close the measured ~60x gap to tree-sitter/pest on the
interpreted path. Milestones in order:

## M1 — Values and captures

- [ ] Emit the shaped value domain: text slices, maps (`Named`), the tagged
      arrays (`seq`/`rep`/`maybe`).
- [ ] Port the flatten laws V0-V4 — the map-beside-array merge is a FLAT
      append and the named context applies to a capture's direct value only
      (both pinned by the corpus; see the B2 findings in
      parsanol-ruby#293). Differential: shaped trees byte-identical to
      `PargArtifact::parse_shape` on the whole conformance corpus.

## M2 — Terminals beyond literals

- [x] **SHIPPED (2026-10-23)**: `Re` emission — the character-class subset
      via the walker's own `CharacterPattern` (byte tests), the general
      case via the `regex` crate (the generated module declares the
      dependency only when a terminal needs it;
      `requires_regex(grammar)` tells the build).
- [ ] Behind-guards (`!<`/`&<`) as end-anchored flanking tests.
- [ ] Predicates (`!`/`&`) as zero-width closures.

## M3 — Injection and rewrite interplay

- [ ] Trivia injection and `atomic` bodies: the artifact already carries
      the rewrite, so generated code needs only the wire atoms — verify no
      compile-time re-derivation sneaks in.
- [ ] C9 tier chains fold identically (the artifact carries plain atoms;
      assert the fold value equals the walker's).

## M4 — Delivery

- [ ] Build integration: a build.rs/CLI that reads a `parsanol-tree/v2`
      artifact and emits the module (artifact in, Rust out — checksums make
      the input trustworthy).
- [ ] Criterion benches: generated vs walker vs VM on the seven-tool
      corpus; publish the closing-gap number next to the honest table.
- [ ] Decide the shipping shape (owner decision): a proc-macro
      (`parg_parser!(artifact)`), a build-time crate, or both.

## Acceptance (bar for "the backend exists")

The whole conformance corpus replays byte-identically through generated
parsers in CI, and the benchmark table shows the generated backend's
number beside the interpreted one.
