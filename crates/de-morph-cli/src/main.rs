//! `de-morph` — German morphological analyzer command-line interface.
//!
//! A single binary with subcommands wrapping the runtime analyzer and the
//! evaluation / diagnostic tooling. The lexicon is embedded at compile time
//! (see `cli/loader.rs`), so the binary is self-contained — no `data/`
//! directory is needed at runtime for analysis. The corpus-driven eval
//! subcommands still take corpus paths as arguments.
//!
//! Build the lexicon first (needed at compile time):
//!   cargo run --release --features extractor --bin build-lexicon
//!
//! Then build/run the CLI:
//!   cargo run --release --bin de-morph -- analyze "Ich gehe zur Schule."
//!
//! Subcommands:
//!   analyze         analyze words / sentences / stdin (default showcase)
//!   split           show compound splittings for words
//!   bench           throughput + memory benchmark
//!   dump            canonical analysis dump (format-change regression)
//!   eval            evaluate against CoNLL-U gold corpora
//!   eval-split      validate the compound splitter vs Wiktionary
//!   dump-unmatched  dump unmatched (surface, lemma, pos) triples to JSONL

#[path = "cli/analyze.rs"]
mod analyze;
#[path = "cli/bench.rs"]
mod bench;
#[path = "cli/dump.rs"]
mod dump;
#[path = "cli/dump_unmatched.rs"]
mod dump_unmatched;
#[path = "cli/eval.rs"]
mod eval;
#[path = "cli/eval_split.rs"]
mod eval_split;
#[path = "cli/loader.rs"]
mod loader;
#[path = "cli/split.rs"]
mod split;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str);
    let rest: Vec<String> = args.iter().skip(2).cloned().collect();

    let result = match cmd {
        Some("analyze") => analyze::run(&rest),
        Some("split") => split::run(&rest),
        Some("bench") => bench::run(&rest),
        Some("dump") => dump::run(&rest),
        Some("eval") => eval::run(&rest),
        Some("eval-split") => eval_split::run(&rest),
        Some("dump-unmatched") => dump_unmatched::run(&rest),
        Some("-h") | Some("--help") | Some("help") | None => {
            print_help();
            return;
        }
        Some(other) => {
            eprintln!("de-morph: unknown command '{other}'\n");
            print_help();
            std::process::exit(2);
        }
    };

    if let Err(e) = result {
        eprintln!("de-morph: error: {e}");
        std::process::exit(1);
    }
}

fn print_help() {
    eprintln!(
        "de-morph — German morphological analyzer

USAGE:
    de-morph <command> [args...]

COMMANDS:
    analyze [--swiss] [--no-oov] [SENTENCE | -]
            Analyze words. No args → curated showcase; SENTENCE → tokenize
            and analyze each word; `-` or piped stdin → one sentence per line.

    split [WORD...]
            Show ranked compound splittings. No args → built-in samples.

    bench [sweep [passes] | load | loadbytes]
            Throughput + memory benchmark. Run under `/usr/bin/time -l`
            to capture max RSS.

    dump
            Print every surface's canonical analysis set (sorted), one line
            per surface. Diff two dumps to prove a format change is lossless.

    eval <path>...
            Evaluate against CoNLL-U gold data (path = .conllu file or dir).
            Reports coverage, lemma/POS/joint accuracy, per-POS breakdown.

    eval-split [compounds.jsonl]
            Validate the compound splitter against Wiktionary's curated
            decomposition. Defaults to data/wiktionary/processed/compounds.jsonl.

    dump-unmatched <path>...
            Dump unmatched (surface, gold_lemma, gold_pos) triples to
            data/lexicon/unmatched.jsonl, sorted by frequency.

  The lexicon is loaded from data/lexicon/ (override with
  DE_MORPH_LEXICON_DIR). Build it with the separate `de-morph-build` tool."
    );
}
