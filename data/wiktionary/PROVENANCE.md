# Provenance — German Wiktionary

## Source

- Name: German Wiktionary (`de.wiktionary.org`)
- Canonical dump URL:
  <https://dumps.wikimedia.org/dewiktionary/20260601/dewiktionary-20260601-pages-articles.xml.bz2>
- Snapshot version: **20260601**
- Fetch date: **2026-06-16**
- Raw artefact sha256:
  `daed03b88f52175c13c742876793894b73d0edf1d3eb946463256f23bb0906e5`
- Raw artefact size: 265,133,447 bytes (~252 MiB compressed)

## Structured record

    id              = "dewiktionary-20260601"
    kind            = "data"
    name            = "German Wiktionary (dump)"
    url             = "https://dumps.wikimedia.org/dewiktionary/20260601/dewiktionary-20260601-pages-articles.xml.bz2"
    version         = "20260601"
    fetch_date      = "2026-06-16"
    license_spdx    = "CC-BY-SA-4.0"
    license_file    = "LICENSES/CC-BY-SA-4.0.txt"
    attribution     = "Wiktionary contributors"
    attribution_url = "https://de.wiktionary.org/"
    sha256          = "daed03b88f52175c13c742876793894b73d0edf1d3eb946463256f23bb0906e5"

## Licence and usage tiers

- Text content: **CC BY-SA 4.0**
  (<https://creativecommons.org/licenses/by-sa/4.0/>; verbatim text at
  `LICENSES/CC-BY-SA-4.0.txt`)
- Older revisions may also carry a GFDL grant; CC BY-SA 4.0 covers
  contemporary edits and is the operative licence for our derivative
  per Wikimedia Foundation Terms of Use, section 7.
- Required attribution: "Wiktionary contributors" with a link back to
  the article (or article history) on `de.wiktionary.org` (Wikimedia
  Foundation Terms of Use, section 7). Recorded authoritatively in the
  root `NOTICE`.

Usage tiers (see `data/README.md`):

- **raw dump** (`raw/…xml.bz2`) — `build-only`. Gitignored; consumed at
  build time only; never shipped.
- **derived lexicon** (`data/lexicon/lexicon.{fst,dat}`) — `ship`. A
  derivative of the snapshot, so it inherits CC BY-SA 4.0. It is **not**
  part of the MIT cargo crate (`data/*` is excluded in `Cargo.toml`); it
  is distributed separately, bundled with its licence and attribution by
  `de-morph-build package`. The MIT-licensed Rust source contains no
  Wiktionary-derived text.
- intermediate `processed/*.jsonl` — `build-only` (regenerable;
  gitignored). Same CC BY-SA status as the lexicon if ever shipped.

## Extraction scope (implemented)

`(lemma, pos, features, source)` records are extracted from German
Wiktionary's standard template families by `de-morph-build extract
<kind>` (the `de-morph-build` workspace crate):

- `{{Deutsch Substantiv Übersicht}}` → `extract nouns`
- conjugation tables → `extract verbs`
- `{{Deutsch Adjektiv Übersicht}}` → `extract adjectives`
- `{{Pronomina-Tabelle}}` / `{{Deutsch Pronomen Übersicht}}` + indeclinable
  indefinites → `extract pronouns` (demonstratives, relatives, and the
  open-ended indefinite/determiner gap: `allerlei` and the `-lei` family,
  `derjenige`, `irgendein`, `jeglicher`, …)
- adverbs / particles / abbreviations / proper nouns → `extract <kind>`
- compound surfaces → `extract compounds` (built for the runtime
  splitter; not baked into the FST)

The core closed class (personal pronouns, articles, prepositions,
conjunctions, numerals, punctuation) comes from the hand-curated table in
`src/paradigm/closed_class.rs`, because the personal-pronoun and possessive
paradigms use parameterless meta-templates whose forms are absent from the
page wikitext. `extract pronouns` skips every lemma that table already
covers, so the two sources never collide — extraction only *adds* the
open-ended pronoun/determiner items the hand-curated set omits.

Each output record carries a `source` tier (`Attested` / `Inflected` /
`Composed` / `Predicted`) so downstream attribution and confidence are
mechanically available.

## Build and reproducibility

The lexicon is deterministic given this snapshot:

    bash scripts/fetch/dewiktionary.sh                  # fetch + verify (sha256 above)
    cargo run -p de-morph-build --release -- all        # extract → build → verify
    cargo run -p de-morph-build --release -- package    # CC BY-SA bundle for shipping

`de-morph-build all` reproduces `data/lexicon/lexicon.{fst,dat}`
byte-for-byte and asserts the lossless analysis fingerprint:

- surfaces: 711,398
- analysis-dump sha256:
  `391c4931061a2ed8e9349b840b699d7080a743f2748fdc9655d959b94ede60d6`

## Files in this directory

- `raw/dewiktionary-20260601-pages-articles.xml.bz2` — gitignored
  snapshot (see `.gitignore`).
- `processed/*.jsonl` — gitignored, regenerable extractor outputs.

## How to refresh

    bash scripts/fetch/dewiktionary.sh

The fetch script downloads the snapshot, verifies the sha256 recorded
in this file, and writes to `data/wiktionary/raw/`. To pin a
different snapshot:

    DUMP_DATE=20260620 bash scripts/fetch/dewiktionary.sh

Then re-run `de-morph-build all`; if the snapshot changed, update the
hashes above (raw sha256 and the lossless fingerprint).
