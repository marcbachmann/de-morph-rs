//! Pinned snapshot configuration, shared by the `all` pipeline and the
//! `package` step. Mirrors the constants formerly hard-coded in
//! `scripts/build/{lexicon,package-data}.sh`.

/// German Wiktionary dump date this build is pinned to.
pub const DUMP_DATE: &str = "20260601";

/// Upstream dump URL (also used in attribution + provenance).
pub const DUMP_URL: &str =
    "https://dumps.wikimedia.org/dewiktionary/20260601/dewiktionary-20260601-pages-articles.xml.bz2";

/// sha256 of the raw `*-pages-articles.xml.bz2` snapshot.
pub const RAW_SHA256: &str = "daed03b88f52175c13c742876793894b73d0edf1d3eb946463256f23bb0906e5";

/// sha256 of the canonical analysis dump over the built lexicon — the
/// lossless fingerprint. Override at runtime with `EXPECTED_DUMP_SHA256`
/// (empty disables the gate, e.g. when intentionally changing contents).
pub const EXPECTED_DUMP_SHA256: &str =
    "391c4931061a2ed8e9349b840b699d7080a743f2748fdc9655d959b94ede60d6";

/// On-disk lexicon format major version (see `src/lexicon/format.rs`).
pub const FORMAT_VERSION: &str = "7";

/// Default raw dump path.
pub fn dump_path() -> String {
    format!("data/wiktionary/raw/dewiktionary-{DUMP_DATE}-pages-articles.xml.bz2")
}

pub const FST_OUT: &str = "data/lexicon/lexicon.fst";
pub const DAT_OUT: &str = "data/lexicon/lexicon.dat";

/// Extraction kinds, in pipeline order. `build` ingests all but
/// `compounds`, which feeds the runtime splitter and is not baked in.
pub const EXTRACTORS: &[&str] = &[
    "nouns",
    "verbs",
    "adjectives",
    "adverbs",
    "particles",
    "abbreviations",
    "propn",
    "pronouns",
    "compounds",
];
