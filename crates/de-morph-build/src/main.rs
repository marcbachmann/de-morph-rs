//! `de-morph-build` — build-time lexicon tooling for de-morph.
//!
//! One binary covering the whole data pipeline that used to live in
//! `scripts/build/*.sh`:
//!
//!   de-morph-build extract <kind>   extract one POS to JSONL
//!   de-morph-build build            build the FST + side table from JSONL
//!   de-morph-build all              fetch-verify → extract all → build → verify
//!   de-morph-build package          stage the CC BY-SA data bundle (tar.gz)
//!
//! Kept separate from the `de-morph` runtime binary so the published
//! `de-morph` library never pulls the bz2/XML/serde dependencies used
//! here. The runtime binary loads the lexicon from disk; it embeds
//! nothing, keeping the MIT crate free of CC BY-SA Wiktionary bytes.

mod config;
mod wiktionary;

mod build_lexicon;
mod extract;
mod package;
mod pipeline;

use anyhow::Result;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str);
    let rest: Vec<String> = args.iter().skip(2).cloned().collect();

    let result: Result<()> = match cmd {
        Some("extract") => extract::run(&rest),
        Some("build") => build_lexicon::run(&rest),
        Some("all") => pipeline::run(&rest),
        Some("package") => package::run(&rest),
        Some("-h") | Some("--help") | Some("help") | None => {
            print_help();
            return;
        }
        Some(other) => {
            eprintln!("de-morph-build: unknown command '{other}'\n");
            print_help();
            std::process::exit(2);
        }
    };

    if let Err(e) = result {
        eprintln!("de-morph-build: error: {e:#}");
        std::process::exit(1);
    }
}

fn print_help() {
    eprintln!(
        "de-morph-build — build-time lexicon tooling for de-morph

USAGE:
    de-morph-build <command> [args...]

COMMANDS:
    extract <kind> [--input PATH] [--output PATH] [--limit N]
            Extract one POS from a Wiktionary dump to JSONL. Kinds: nouns
            verbs adjectives adverbs particles abbreviations propn pronouns
            compounds.

    build [--input PATH]... [--fst-out PATH] [--dat-out PATH]
            Build the runtime FST + side table from extracted JSONL.

    all [--skip-extract]
            Full reproducible pipeline: verify the pinned dump sha256,
            extract every POS, build the lexicon, then verify the lossless
            analysis fingerprint. --skip-extract reuses existing JSONL.

    package
            Stage + tar the CC BY-SA 4.0 data bundle (lexicon + licence +
            attribution + provenance) under dist/, re-verifying the
            fingerprint first."
    );
}
