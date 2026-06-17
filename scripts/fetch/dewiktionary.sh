#!/usr/bin/env bash
# Fetch a German Wiktionary dump snapshot.
#
# Provenance: German Wiktionary text content is licensed under CC BY-SA 4.0
# (with a separate GFDL grant for some older revisions). The lexicon derived
# from this dump will inherit CC BY-SA 4.0 and must preserve attribution to
# Wiktionary contributors. See data/wiktionary/PROVENANCE.md for the
# per-snapshot record.
#
# The default DUMP_DATE pins one specific snapshot for reproducibility. Do
# NOT use "latest" — the latest symlink moves and breaks the provenance
# chain.
#
# Usage:
#   bash scripts/fetch/dewiktionary.sh                      # default snapshot
#   DUMP_DATE=20260601 bash scripts/fetch/dewiktionary.sh   # override snapshot

set -euo pipefail

DUMP_DATE="${DUMP_DATE:-20260601}"
EXPECTED_SHA256="${EXPECTED_SHA256:-}"

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd -- "${ROOT}/.." && pwd)"
DEST_DIR="${ROOT}/data/wiktionary/raw"
DEST="${DEST_DIR}/dewiktionary-${DUMP_DATE}-pages-articles.xml.bz2"
URL="https://dumps.wikimedia.org/dewiktionary/${DUMP_DATE}/dewiktionary-${DUMP_DATE}-pages-articles.xml.bz2"

mkdir -p "${DEST_DIR}"

sha256_of() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

verify_hash() {
    local file="$1"
    local actual
    actual="$(sha256_of "${file}")"
    if [[ -n "${EXPECTED_SHA256}" && "${actual}" != "${EXPECTED_SHA256}" ]]; then
        printf 'ERROR: hash mismatch for %s\n  expected=%s\n  got     =%s\n' \
            "${file}" "${EXPECTED_SHA256}" "${actual}" >&2
        return 1
    fi
    printf '%s' "${actual}"
}

if [[ -f "${DEST}" ]]; then
    actual="$(verify_hash "${DEST}")"
    printf 'OK (cached): %s\n  sha256=%s\n' "${DEST}" "${actual}"
    exit 0
fi

printf 'GET %s\n -> %s\n' "${URL}" "${DEST}"
curl --fail --show-error --location \
     --user-agent 'de-morph-rs dewiktionary-fetch (Marc Bachmann; marc@livingdocs.io)' \
     --output "${DEST}" \
     "${URL}"

actual="$(verify_hash "${DEST}")"
size="$(wc -c < "${DEST}" | tr -d ' ')"
printf '\nDone.\n  size  =%s bytes\n  sha256=%s\n' "${size}" "${actual}"

cat <<EOF

Record in data/wiktionary/PROVENANCE.md:

  id              = "dewiktionary-${DUMP_DATE}"
  kind            = "data"
  name            = "German Wiktionary (dump)"
  url             = "${URL}"
  version         = "${DUMP_DATE}"
  fetch_date      = "$(date +%Y-%m-%d)"
  license_spdx    = "CC-BY-SA-4.0"
  license_file    = "LICENSES/CC-BY-SA-4.0.txt"
  attribution     = "Wiktionary contributors"
  attribution_url = "https://de.wiktionary.org/"
  usage           = "build-only"
  sha256          = "${actual}"
EOF
