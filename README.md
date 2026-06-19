# de-morph-rs

German morphological analyzer based on finite-state transducers, written
in Rust.

Status: **pre-alpha**. The repository contains project scaffolding, an
attribution discipline, a curated literature index, and a working
analyzer: FST-backed lexicon lookup with fallbacks for Swiss `ss`/`ß`
orthography, hyphenated compounds, and out-of-vocabulary guessing
(noun/verb/adjective paradigms). APIs and on-disk formats may still
change.

## Licensing model

This project separates code and data licenses by design.

- **Source code** is licensed under MIT (see `LICENSE-MIT`). Reading
  papers (open or closed) to implement algorithms is unrestricted —
  copyright does not cover methods.
- **Data files** distributed with or referenced by this crate may carry
  separate licenses. The most likely shipped data layer derives from
  German Wiktionary (CC BY-SA 4.0); derivatives must remain CC BY-SA 4.0
  and ship as a separately-licensed artifact. Commercial users who
  cannot accept CC BY-SA on a data artifact should use the (planned)
  "bring your own data" build path.

Verbatim third-party license texts live under `LICENSES/`. End-user attribution
text is collected in `NOTICE`.

## Layout

    Cargo.toml         library manifest (MIT)
    src/               Rust source (MIT)
    data/              data sources, each with its own PROVENANCE.md
        wiktionary/    primary lexicon source (CC BY-SA 4.0 when populated)
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


## Building

    cargo build

The published crate bundles no data (`data/` is excluded from the
package — see `exclude` in `Cargo.toml`). With no lexicon loaded the
analyzer still returns best-effort out-of-vocabulary guesses; for real
coverage, build a lexicon and open it with `Analyzer::open`. A
Wiktionary-derived lexicon lives under `data/lexicon/` in the repo for
development and evaluation.

## Contributing

See `CONTRIBUTING.md` for the data-sourcing policy. Briefly: a new
source requires a `PROVENANCE.md` in the
source's data subdirectory, and the verbatim license text in
`LICENSES/` — added *before* any data lands.
