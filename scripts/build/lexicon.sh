#!/usr/bin/env bash
# Build the runtime lexicon (lexicon.fst + lexicon.dat) from the pinned
# German Wiktionary snapshot, reproducibly.
#
# Provenance / licence:
#   Input  : data/wiktionary/raw/dewiktionary-20260601-pages-articles.xml.bz2
#            German Wiktionary, CC BY-SA 4.0 (see data/wiktionary/PROVENANCE.md).
#   Output : data/lexicon/lexicon.{fst,dat} — a DERIVATIVE of the snapshot and
#            therefore CC BY-SA 4.0 itself. It is NOT part of the MIT-licensed
#            cargo crate (data/ is excluded in Cargo.toml). Ship it separately
#            with scripts/build/package-data.sh, which attaches the licence and
#            attribution. The Rust source stays MIT; only this data layer is
#            copyleft.
#
# Pipeline (matches src/bin/build-lexicon.rs defaults exactly):
#   1. verify the raw dump sha256 against PROVENANCE
#   2. cargo build --release --features extractor (extractors + build-lexicon
#      + the dump_analyses verification harness)
#   3. run the 8 extract-* binaries  -> data/wiktionary/processed/*.jsonl
#   4. run build-lexicon (7 ingested inputs; compounds.jsonl is produced for
#      the runtime splitter but intentionally not baked into the FST)
#   5. dump every analysis and verify the lossless fingerprint is unchanged
#
# The build is deterministic: given this snapshot, steps 3-4 reproduce
# data/lexicon/lexicon.{fst,dat} byte-for-byte, and the analysis dump hashes
# to EXPECTED_DUMP_SHA256 below.
#
# Usage:
#   bash scripts/build/lexicon.sh                 # full: extract + build + verify
#   bash scripts/build/lexicon.sh --skip-extract  # rebuild from existing JSONL only
#
# Env:
#   EXPECTED_DUMP_SHA256  override the lossless-dump acceptance hash (default
#                         pins the 20260601 snapshot result). Set empty to skip
#                         the lossless gate (e.g. when intentionally changing
#                         the lexicon contents).

set -euo pipefail

# --- configuration (pinned to the snapshot recorded in PROVENANCE) ----------
DUMP_DATE="20260601"
RAW_SHA256="daed03b88f52175c13c742876793894b73d0edf1d3eb946463256f23bb0906e5"
EXPECTED_DUMP_SHA256="${EXPECTED_DUMP_SHA256-387e7c6f3799774788af85c52bea7708d13fada7ce86fd5688985fe42f271be5}"

# Extractors to run. build-lexicon ingests all but `compounds`, which is built
# for the runtime compound splitter and intentionally not baked into the FST.
EXTRACTORS=(nouns verbs adjectives adverbs particles abbreviations propn pronouns compounds)

# --- locate the repo root ---------------------------------------------------
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT}"

DUMP="data/wiktionary/raw/dewiktionary-${DUMP_DATE}-pages-articles.xml.bz2"
FST_OUT="data/lexicon/lexicon.fst"
DAT_OUT="data/lexicon/lexicon.dat"

SKIP_EXTRACT=0
[[ "${1-}" == "--skip-extract" ]] && SKIP_EXTRACT=1

sha256_of() {
    if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
    else sha256sum "$1" | awk '{print $1}'; fi
}

# Hash stdin (portable: prefer shasum on macOS, sha256sum elsewhere).
sum_stdin() {
    if command -v shasum >/dev/null 2>&1; then shasum -a 256; else sha256sum; fi | awk '{print $1}'
}

# --- 1. verify the raw dump -------------------------------------------------
if [[ ! -f "${DUMP}" ]]; then
    printf 'ERROR: raw dump missing: %s\n  run: bash scripts/fetch/dewiktionary.sh\n' "${DUMP}" >&2
    exit 1
fi
actual="$(sha256_of "${DUMP}")"
if [[ "${actual}" != "${RAW_SHA256}" ]]; then
    printf 'ERROR: raw dump hash mismatch\n  expected=%s\n  got     =%s\n' "${RAW_SHA256}" "${actual}" >&2
    exit 1
fi
printf '[1/5] raw dump verified  sha256=%s\n' "${actual}"

# --- 2. build the toolchain -------------------------------------------------
printf '[2/5] building toolchain (cargo --release --features extractor) ...\n'
cargo build --release --features extractor --bins --example dump_analyses

# --- 3. extract -------------------------------------------------------------
if [[ "${SKIP_EXTRACT}" -eq 1 ]]; then
    printf '[3/5] --skip-extract: reusing existing data/wiktionary/processed/*.jsonl\n'
else
    mkdir -p data/wiktionary/processed
    for x in "${EXTRACTORS[@]}"; do
        printf '[3/5] extract-%s ...\n' "${x}"
        "./target/release/extract-${x}" \
            --input "${DUMP}" \
            --output "data/wiktionary/processed/${x}.jsonl"
    done
fi

# --- 4. build the lexicon (build-lexicon defaults) --------------------------
printf '[4/5] build-lexicon ...\n'
mkdir -p data/lexicon
./target/release/build-lexicon --fst-out "${FST_OUT}" --dat-out "${DAT_OUT}"

# --- 5. verify the lossless fingerprint -------------------------------------
dump_sha="$(./target/release/examples/dump_analyses | sum_stdin)"
printf '[5/5] lossless analysis dump sha256=%s\n' "${dump_sha}"
if [[ -n "${EXPECTED_DUMP_SHA256}" && "${dump_sha}" != "${EXPECTED_DUMP_SHA256}" ]]; then
    printf 'ERROR: lossless fingerprint changed\n  expected=%s\n  got     =%s\n' \
        "${EXPECTED_DUMP_SHA256}" "${dump_sha}" >&2
    printf 'If this change is intentional, update EXPECTED_DUMP_SHA256 in this script.\n' >&2
    exit 1
fi

printf '\nDone.\n  %s  (%s bytes)\n  %s  (%s bytes)\n' \
    "${FST_OUT}" "$(wc -c < "${FST_OUT}" | tr -d ' ')" \
    "${DAT_OUT}" "$(wc -c < "${DAT_OUT}" | tr -d ' ')"
printf '  Package for separate CC BY-SA distribution:\n    bash scripts/build/package-data.sh\n'
