# scripts/

Reproducible scripts for fetching and building the data layer. Each
script is deterministic given a fixed upstream snapshot: it downloads
to a known location, verifies the sha256 recorded in the source's
`PROVENANCE.md`, and refuses to continue on mismatch.

## Layout

    scripts/
        fetch/                  one script per upstream source
            <source-id>.sh
        build/                  data processing pipelines

## Conventions

- Shell scripts use `set -euo pipefail` and exit non-zero on hash
  mismatch.
- The upstream URL, expected sha256, and expected size live in the
  script header so that the script can be audited without consulting
  other files.
- Downloads go to `data/<source-id>/raw/` (gitignored). The script
  never writes to `data/<source-id>/processed/`; that is the job of
  a separate build target so that the licence boundary between raw
  and processed remains explicit.
- A script must be idempotent: re-running it must not re-download if
  the destination file already exists and matches the expected hash.

## Adding a fetch script

See `CONTRIBUTING.md` at the project root, step 6.

## Current scripts

- `fetch/dewiktionary.sh` — fetch + sha256-verify the pinned German
  Wiktionary snapshot into `data/wiktionary/raw/`.
- `build/lexicon.sh` — extract → build → verify the runtime lexicon
  (`data/lexicon/lexicon.{fst,dat}`); deterministic, asserts the
  lossless analysis fingerprint.
- `build/package-data.sh` — bundle the built lexicon as a standalone
  CC BY-SA 4.0 distributable (artifact + LICENSE + ATTRIBUTION +
  PROVENANCE + checksums) for shipping separately from the MIT crate.
