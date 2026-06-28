# de-morph-rs

German morphological analyzer based on finite-state transducers, written
in Rust.

Status: **pre-alpha**. A working analyzer: FST-backed lexicon lookup
with fallbacks for Swiss `ss`/`ß` orthography, hyphenated compounds, and
out-of-vocabulary guessing (noun/verb/adjective paradigms). APIs and
on-disk formats may still change.

## Licensing

Source code is MIT (see `LICENSE-MIT`); the published crate ships no
data. Any data layer added later derives from [German
Wiktionary](https://de.wiktionary.org/) and is CC BY-SA 4.0, shipped as
a separate artifact rather than compiled into the MIT source. No GPL,
non-commercial, or academic-only source ever enters the shipped
artifact, even indirectly. `NOTICE` is the attribution record; verbatim
license texts live under `LICENSES/`.

## Layout

    Cargo.toml         library manifest (MIT)
    src/               Rust source (MIT)
    data/              external data + generated build artifacts
        wiktionary/    lexicon source (CC BY-SA 4.0); see PROVENANCE.md
        lexicon/       generated FST + side table (gitignored)
    scripts/           reproducible fetch and build scripts
        fetch/         one script per upstream source
    LICENSES/          verbatim third-party license texts
    NOTICE             project-level third-party attribution
    CONTRIBUTING.md    data-sourcing policy

## Design overview

A precomputed finite-state map from German surface forms to one or
more analyses (lemma + POS + features). The runtime engine uses the
[`fst`](https://crates.io/crates/fst) crate (Daciuk-style minimised
finite-state acceptor over a byte alphabet) for compact, fast lookup;
multiple analyses per surface form are encoded via a side table indexed
by the FST's u64 value.

Construction is offline. The build pipeline will eventually:

1. Extract `(lemma, pos, paradigm)` triples from German Wiktionary.
2. Expand each `(lemma, paradigm)` into surface forms via rule
   application (rules expressed in a small in-repo DSL).
3. Invert to `(surface, analysis)` and bake into an `fst::Map`.
4. Optionally compose with a compound-splitter at runtime for OOV.

When productive compounding or unknown-word guessing become necessary,
construction will move to [`rustfst`](https://crates.io/crates/rustfst)
(pure-Rust OpenFST port, MIT/Apache-2.0) for true transducer composition,
exporting analyzed pairs into the same runtime format.

Design decisions are documented inline in the source. The
part-of-speech and feature inventories follow
[Universal Dependencies](https://universaldependencies.org/) — the UPOS
tag set and the morphological feature inventory.

## Building

    cargo build

The published library bundles no data: `data/` sits at the repo root,
outside every crate, so it is never packaged. With no lexicon loaded the
analyzer still returns best-effort out-of-vocabulary guesses; for real
coverage, build a lexicon and open it with `Analyzer::open`.

## Workspace

A Cargo workspace of three crates under `crates/`, split by role and
dependency weight:

- **`de-morph-core`** — the library (`de_morph`). Depends only on `fst`.
- **`de-morph-cli`** — the runtime CLI: `analyze`, `split`, `bench`, `dump`,
  `eval`, `eval-split`, `dump-unmatched`. It loads the lexicon from
  `data/lexicon/` at runtime (override with `DE_MORPH_LEXICON_DIR`) and
  **embeds nothing**, so the MIT binary carries no CC BY-SA
  Wiktionary-derived bytes.
- **`de-morph-build`** — the build pipeline: `extract <kind>`, `build`,
  `all`, `package`. `all` runs the whole reproducible flow (verify the
  pinned dump → extract every POS → build the FST → verify the lossless
  fingerprint); `package` stages the CC BY-SA 4.0 data bundle. The
  bz2/XML/serde tooling dependencies live only here.

Typical flow:

    bash scripts/fetch/dewiktionary.sh   # fetch + verify the dump
    cargo run -p de-morph-build --release -- all
    cargo run --release -- analyze "Ich gehe zur Schule."

Run `de-morph --help` / `de-morph-build --help` for the full list.

## Contributing

See `CONTRIBUTING.md` for the data-sourcing policy. Briefly: a new
source requires a `PROVENANCE.md` in the source's data subdirectory, an
attribution paragraph in `NOTICE`, and the verbatim license text in
`LICENSES/` — added *before* any data lands.
