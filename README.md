# de-morph-rs

German morphological analyzer based on finite-state transducers, written
in Rust.

Status: **pre-alpha**. A working analyzer: FST-backed lexicon lookup
with fallbacks for Swiss `ss`/`ß` orthography, hyphenated and solid
compounds (with Fugenelemente), and out-of-vocabulary guessing
(noun/verb/adjective paradigms). APIs and on-disk formats may still
change.

## Licensing

Source code is MIT (see `LICENSE-MIT`); the published crate ships no
data. The lexicon derives from [German
Wiktionary](https://de.wiktionary.org/) and is CC BY-SA 4.0, distributed
as a separate artifact (see `dist/`) rather than compiled into the MIT
source. No GPL, non-commercial, or academic-only source ever enters the
shipped artifact, even indirectly. `NOTICE` is the attribution record;
verbatim license texts live under `LICENSES/`.

## Layout

A virtual Cargo workspace; crates live under `crates/`:

    crates/de-morph-core/   library `de_morph` (MIT); deps: fst only
    crates/de-morph-cli/    runtime CLI `de-morph` (MIT); embeds no data
    crates/de-morph-build/  build-time tooling (extraction, build, package)
    data/                   external data + generated build artifacts
        wiktionary/raw/         pinned upstream dump (gitignored)
        wiktionary/processed/   extracted per-POS JSONL + intermediate FSTs
        lexicon/                generated FST + side table (gitignored)
    dist/                   packaged CC BY-SA 4.0 lexicon bundle + tarball
    LICENSES/               verbatim third-party license texts
    NOTICE                  project-level third-party attribution
    CONTRIBUTING.md         data-sourcing policy

The three crates split by role and dependency weight:

- **`de-morph-core`** — the library (`de_morph`). Depends only on `fst`.
  Exposes `Analyzer` (`open`, `from_lexicon`, `empty`, `analyze`,
  `with_oov_fallback`, `with_swiss_orthography`) and `Lexicon`. Part-of-speech
  and feature inventories follow
  [Universal Dependencies](https://universaldependencies.org/) — the UPOS tag
  set and the morphological feature inventory.
- **`de-morph-cli`** — the runtime CLI. Loads the lexicon from
  `data/lexicon/` at runtime (override with `DE_MORPH_LEXICON_DIR`) and
  **embeds nothing**, so the MIT binary carries no CC BY-SA
  Wiktionary-derived bytes.
- **`de-morph-build`** — the offline build pipeline. The bz2/XML/serde/HTTP
  tooling dependencies live only here, so none of it reaches library
  consumers.

## Design overview

A precomputed finite-state map from German surface forms to one or
more analyses (lemma + UPOS + features). The runtime engine uses the
[`fst`](https://crates.io/crates/fst) crate (Daciuk-style minimised
finite-state acceptor over a byte alphabet) for compact, fast lookup;
multiple analyses per surface form are encoded via a side table indexed
by the FST's u64 value.

Construction is offline (`de-morph-build`):

1. Extract `(lemma, pos, paradigm)` data from German Wiktionary, one
   JSONL file per POS.
2. Expand each `(lemma, paradigm)` into surface forms via paradigm rules.
3. Invert to `(surface, analysis)` and bake into an `fst::Map` plus a
   packed side table.

At analysis time the `Analyzer` walks a ranked fallback chain, tagging
each result with its `Source` (most trusted first): direct lexicon hit
(`Attested`/`Inflected`) → Swiss `ss`/`ß` variant (opt-in) → hyphenated
compound → solid compound with linker detection (`Composed`) →
suffix-based OOV paradigm guess (`Predicted`). The result is best-effort
even with no lexicon loaded.

Design decisions are documented inline in the source.

## Using the library

```rust
use de_morph::analyzer::Analyzer;

// Loads data/lexicon/ — or build/download a lexicon first (see below).
let analyzer = Analyzer::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
for analysis in analyzer.analyze("Häuser") {
    println!("{} {:?} {:?}", analysis.lemma, analysis.pos, analysis.source);
}

// Or run fallback-only (OOV guessing, no data):
let analyzer = Analyzer::empty();
```

A prebuilt lexicon ships in `dist/` as a CC BY-SA 4.0 data artifact,
separate from the MIT crate. Construct from owned or `'static` bytes via
`Lexicon::from_bytes` / `Lexicon::from_static`.

## Building

    cargo build

The published library bundles no data: `data/` sits at the repo root,
outside every crate, so it is never packaged. With no lexicon loaded the
analyzer still returns best-effort out-of-vocabulary guesses; for real
coverage, build a lexicon and open it with `Analyzer::open`.

### Build the lexicon

    cargo run -p de-morph-build --release -- all   # fetch (if needed) → build
    cargo run --release -- analyze "Ich gehe zur Schule."

`all` runs the whole reproducible flow: download the pinned dump when
it is missing and verify its sha256 → extract every POS → build the FST
→ verify a lossless analysis fingerprint. `package` then stages and tars
the CC BY-SA 4.0 data bundle under `dist/`.

CLI subcommands:

- `de-morph` — `analyze`, `split`, `bench`, `dump`, `eval`,
  `eval-split`, `dump-unmatched`.
- `de-morph-build` — `extract <kind>`, `build`, `all`, `package`.

Run `de-morph --help` / `de-morph-build --help` for the full list.

## Testing

    cargo test --workspace

CI (GitHub Actions) runs `cargo test --workspace` and a nightly
`cargo fmt --check` on every push to `main` and every pull request.

## Contributing

See `CONTRIBUTING.md` for the data-sourcing policy. Briefly: a new
source requires a `PROVENANCE.md` in the source's data subdirectory, an
attribution paragraph in `NOTICE`, and the verbatim license text in
`LICENSES/` — added *before* any data lands.
