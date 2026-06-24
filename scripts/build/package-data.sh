#!/usr/bin/env bash
# Package the built lexicon as a self-contained, CC BY-SA 4.0 distributable.
#
# Option A (keep code and data separate): the MIT cargo crate ships NO
# Wiktionary-derived bytes (data/ is excluded in Cargo.toml). The lexicon is
# instead shipped as the tarball this script produces — a standalone artifact
# that carries its own licence, attribution, and provenance so the CC BY-SA
# obligations (attribution + ShareAlike) travel with the bytes wherever they go
# (GitHub release asset, object store, etc.). Consumers load it at runtime via
# `Lexicon::from_bytes` / `Lexicon::from_static`.
#
# The bundle contains:
#   lexicon.fst, lexicon.dat   the artifact
#   LICENSE                    verbatim CC BY-SA 4.0
#   ATTRIBUTION                Wiktionary-contributors credit (licence clause)
#   PROVENANCE                 snapshot id, dump URL + sha256, lossless fingerprint
#   README.md                  what it is + how to load it
#   SHA256SUMS                 checksums of the artifact files
#
# Before packaging, the script re-verifies the lossless fingerprint so a stale
# or hand-edited artifact can never be shipped.
#
# Usage:
#   bash scripts/build/package-data.sh

set -euo pipefail

DUMP_DATE="20260601"
DUMP_URL="https://dumps.wikimedia.org/dewiktionary/${DUMP_DATE}/dewiktionary-${DUMP_DATE}-pages-articles.xml.bz2"
RAW_SHA256="daed03b88f52175c13c742876793894b73d0edf1d3eb946463256f23bb0906e5"
EXPECTED_DUMP_SHA256="387e7c6f3799774788af85c52bea7708d13fada7ce86fd5688985fe42f271be5"
FORMAT_VERSION="7"   # src/lexicon/format.rs VERSION_MAJOR

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT}"

FST="data/lexicon/lexicon.fst"
DAT="data/lexicon/lexicon.dat"
[[ -f "${FST}" && -f "${DAT}" ]] || {
    printf 'ERROR: lexicon not built. Run: bash scripts/build/lexicon.sh\n' >&2; exit 1; }

CRATE_VERSION="$(awk -F\" '/^version *= *"/{print $2; exit}' Cargo.toml)"
PKG="de-morph-lexicon-v${CRATE_VERSION}-dewiktionary-${DUMP_DATE}"
STAGE="dist/${PKG}"

sumcmd() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }

# --- re-verify the lossless fingerprint before shipping ---------------------
printf 'Verifying lossless fingerprint ...\n'
cargo build --release --features extractor --example dump_analyses >/dev/null 2>&1
dump_sha="$(./target/release/examples/dump_analyses | sumcmd | awk '{print $1}')"
if [[ "${dump_sha}" != "${EXPECTED_DUMP_SHA256}" ]]; then
    printf 'ERROR: artifact does not match the pinned lossless fingerprint\n  expected=%s\n  got     =%s\nRebuild with scripts/build/lexicon.sh\n' \
        "${EXPECTED_DUMP_SHA256}" "${dump_sha}" >&2
    exit 1
fi
printf '  ok  sha256=%s\n' "${dump_sha}"

# --- stage the bundle -------------------------------------------------------
rm -rf "${STAGE}"
mkdir -p "${STAGE}"
cp "${FST}" "${DAT}" "${STAGE}/"
cp LICENSES/CC-BY-SA-4.0.txt "${STAGE}/LICENSE"

cat > "${STAGE}/ATTRIBUTION" <<EOF
This artifact is a derivative of the German Wiktionary and is licensed under
the Creative Commons Attribution-ShareAlike 4.0 International licence
(CC BY-SA 4.0); see LICENSE.

Attribution (required by CC BY-SA 4.0, section 3(a)):

    "German Wiktionary contributors", ${DUMP_URL%/*}/
    Source: ${DUMP_URL}
    Article histories: https://de.wiktionary.org/

ShareAlike: if you remix, transform, or build upon this artifact, you must
distribute your contributions under CC BY-SA 4.0. This obligation attaches to
the data only; it does not extend to software that merely loads it.
EOF

cat > "${STAGE}/PROVENANCE" <<EOF
de-morph lexicon — provenance
=============================

artifact          : lexicon.fst + lexicon.dat
on-disk format    : v${FORMAT_VERSION} (de-morph-rs lexicon format)
built by          : de-morph-rs v${CRATE_VERSION}

source            : German Wiktionary (de.wiktionary.org)
snapshot          : ${DUMP_DATE}
dump url          : ${DUMP_URL}
dump sha256       : ${RAW_SHA256}
licence           : CC BY-SA 4.0 (see LICENSE, ATTRIBUTION)

lossless fingerprint (sha256 of the canonical analysis dump over all surfaces):
                    ${EXPECTED_DUMP_SHA256}

Reproduce from the dump:
    bash scripts/fetch/dewiktionary.sh        # fetch + verify the snapshot
    bash scripts/build/lexicon.sh             # extract + build + verify
    bash scripts/build/package-data.sh        # produce this bundle
EOF

cat > "${STAGE}/README.md" <<EOF
# de-morph lexicon (data artifact)

Prebuilt morphological lexicon for [de-morph-rs][crate], derived from the
German Wiktionary snapshot \`${DUMP_DATE}\`.

**Licence: CC BY-SA 4.0** (see \`LICENSE\` and \`ATTRIBUTION\`). This data
artifact is *separate* from the de-morph-rs crate, which is MIT-licensed and
contains none of these bytes. Using this artifact subjects the data — not your
code — to CC BY-SA's attribution and ShareAlike terms.

## Files

| file          | description                                  |
|---------------|----------------------------------------------|
| \`lexicon.fst\` | surface → packed-pointer FST (format v${FORMAT_VERSION})       |
| \`lexicon.dat\` | lemma table + packed analysis records        |
| \`SHA256SUMS\`  | checksums of the two files above             |

## Use

\`\`\`rust
// Owns the bytes (e.g. read from disk at startup):
let lex = de_morph::Lexicon::from_bytes(fst_bytes, dat_bytes)?;

// Or zero-copy from 'static bytes (e.g. include_bytes! in your own,
// CC-BY-SA-aware, binary):
let lex = de_morph::Lexicon::from_static(FST, DAT)?;
\`\`\`

Verify integrity: \`sha256sum -c SHA256SUMS\`.

[crate]: https://crates.io/crates/de-morph-rs
EOF

( cd "${STAGE}" && sumcmd lexicon.fst lexicon.dat > SHA256SUMS )

# --- tarball ----------------------------------------------------------------
TARBALL="dist/${PKG}.tar.gz"
tar -C dist -czf "${TARBALL}" "${PKG}"
sumcmd "${TARBALL}" > "${TARBALL}.sha256"

printf '\nPackaged CC BY-SA 4.0 data bundle:\n'
printf '  %s  (%s bytes)\n' "${TARBALL}" "$(wc -c < "${TARBALL}" | tr -d ' ')"
printf '  staged tree: %s/\n' "${STAGE}"
printf '\nContents:\n'
( cd "${STAGE}" && ls -1 )
printf '\nReady to attach to a GitHub release or object store. The MIT crate\n'
printf 'ships none of these bytes (verify: cargo package --list | grep -E "^(data|dist)/").\n'
