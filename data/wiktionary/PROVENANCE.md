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

## Licence

- Text content: **CC BY-SA 4.0**
  (<https://creativecommons.org/licenses/by-sa/4.0/>; verbatim text at
  `LICENSES/CC-BY-SA-4.0.txt`)
- Older revisions may also carry a GFDL grant; CC BY-SA 4.0 covers
  contemporary edits and is the operative licence for our derivative
  per Wikimedia Foundation Terms of Use, section 7.
- Required attribution: "Wiktionary contributors" with a link back to
  the article (or article history) on `de.wiktionary.org` (Wikimedia
  Foundation Terms of Use, section 7).
- Implication for this project: any artefact derived from this
  snapshot — the processed lexicon, the FST data file, any compiled
  morphological table — inherits CC BY-SA 4.0 and is shipped as a
  separately-licensed file rather than as part of the MIT-licensed
  Rust source.

## Extraction scope

Planned (not yet implemented): extraction of `(lemma, pos,
inflection-table)` triples from German Wiktionary's standard
template families:

- `{{Deutsch Substantiv Übersicht}}` and related noun tables
- `{{Deutsch Verb Übersicht}}` and conjugation pages
- `{{Deutsch Adjektiv Übersicht}}` for adjective declension
- Closed-class tables for pronouns, determiners, prepositions

The extraction proceeds at three levels of trust:

1. **Strict** — only entries with a complete, well-formed template.
2. **Permissive** — entries with partial templates, completed by
   paradigm inference (rules expressed in the in-repo DSL).
3. **Heuristic** — entries with no template but with section-level
   POS information; output flagged as low-confidence.

Each output entry carries a provenance pointer back to its source
Wiktionary article and revision so that downstream attribution is
mechanically possible.

## Files in this directory

- `raw/dewiktionary-20260601-pages-articles.xml.bz2` — gitignored
  snapshot from the dump (see `.gitignore`).
- `processed/` — versioned, attributed processed outputs (none yet;
  the extractor has not been written).

## How to refresh

    bash scripts/fetch/dewiktionary.sh

The fetch script downloads the snapshot, verifies the sha256 recorded
in this file, and writes to `data/wiktionary/raw/`. To pin a
different snapshot:

    DUMP_DATE=20260620 bash scripts/fetch/dewiktionary.sh

The processing pipeline (separate target, not yet implemented) reads
`raw/`, produces `processed/`, and updates this file with the new
snapshot date and hash.
