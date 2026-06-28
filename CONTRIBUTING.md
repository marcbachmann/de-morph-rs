# Contributing — data sourcing policy

This project keeps a strict provenance discipline so that downstream
consumers can rely on the licence claims in the manifest. New data
sources must be added in the order below. Skipping any step is a
ground for revert.

## 1. Verify the licence is acceptable

A source is acceptable **for shipping** if and only if its licence is
one of:

- Public domain / CC0
- A permissive licence: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause,
  ISC, Unlicense, Unicode
- CC BY 4.0 (attribution-only; compatible with MIT shipping if
  attributed in `NOTICE`)
- CC BY-SA 4.0 — shipped only as a separately-licensed data artifact,
  never compiled into MIT-licensed code, and only with the upstream
  attribution preserved

A source is **not** acceptable for shipping (but may be used eval-only
at arm's length) if its licence is:

- GPL / LGPL / AGPL / MPL of any version (copyleft propagates)
- "Academic use only", "research only", "non-commercial"
- Any CC licence with NC (NonCommercial) or ND (NoDerivatives)
- Unclear, unstated, or asserted by upstream but not visible in the
  artifact itself

If in doubt: **not acceptable**. Open an issue and ask before fetching.

## 2. Decide the usage tier

- `ship` — bundled with or distributed by the crate. Inherits the
  source's licence; attribution required in `NOTICE`.
- `build-only` — used at build time only. Outputs ship **only** if
  those outputs are not derivatives under the source's licence. If in
  doubt, treat as `ship`.
- `eval-only` — used only to measure quality against a reference;
  never enters the shipped artifact, directly or indirectly.

## 3. Record the provenance

Create `data/<source-id>/PROVENANCE.md` with all fields populated:
upstream URL, version, fetch date, licence (SPDX), usage tier, the raw
artefact's sha256, and a one-paragraph summary of what is extracted.
This goes in **before** any data file lands. A future CI check will
reject commits that add files under `data/<source-id>/` without it.

## 4. Add the licence text and attribution

- Put the verbatim licence text under `LICENSES/<SPDX>.txt` (download
  from the canonical authority — opensource.org, spdx.org,
  creativecommons.org).
- Add the attribution paragraph to `NOTICE`.

## 5. Add the data

- Raw downloads go under `data/<source-id>/raw/` and are gitignored.
- Processed outputs go under `data/<source-id>/processed/`.

## 6. Reproducible fetch

Pin the source in `crates/de-morph-build/src/config.rs` — a dated
snapshot URL and its sha256 — and have the build pipeline download the
raw artefact into `data/<source-id>/raw/` when missing, then verify the
hash before use (see `pipeline.rs::download_dump`). Never pin a "latest"
URL; it moves and breaks the provenance chain. CI (when set up) re-runs
the pipeline to catch upstream tampering and keep the snapshot
reproducible.

## Methods from papers

Reading a paper to learn a method is unrestricted regardless of the
paper's licence — algorithms are not subject to copyright. Citations
belong in source comments. Implementations are ours, and only ours, end
to end.

## Eval-only discipline

Code that uses `eval-only` data lives under `tests/eval/` or its own
binary target. It MUST NOT be reachable from the published library
target. Reviewers should treat any `use de_morph::...` that pulls in
an `eval-only` path as a release blocker.
