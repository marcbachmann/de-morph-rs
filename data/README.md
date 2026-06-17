# data/

External data, organised by upstream source. Every subdirectory
corresponds to a single source and follows the same layout:

    data/<source-id>/
        PROVENANCE.md       human-readable provenance record
        raw/                gitignored — fetched-as-is upstream snapshot
        processed/          versioned, attributed processed outputs

Each `<source-id>` directory carries its own `PROVENANCE.md`.

## Usage tiers

Each source declares a `usage` tier in its `PROVENANCE.md`:

- `ship` — bundled with or distributed by the crate. The source's
  licence attaches to the shipped artefact; attribution required in
  `NOTICE`.
- `build-only` — used at build time only; outputs ship only if they
  are not derivatives under the source's licence. When in doubt,
  treat as `ship`.
- `eval-only` — used only to measure quality. **MUST NOT enter the
  shipped artefact, directly or indirectly.** Lives under
  `data/eval/` (or carries the `eval-only` tier explicitly) and is
  reachable only from `tests/eval/` or a dedicated binary target.

## Current sources

(none yet)

## Adding a source

See `CONTRIBUTING.md` at the project root. Order of operations is
non-negotiable: licence verification, `PROVENANCE.md`, licence
text in `LICENSES/`, attribution in `NOTICE`, fetch script, then
finally the data.
