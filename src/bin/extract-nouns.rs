//! Extract German noun analyses from a Wiktionary dump.
//!
//! Output is JSON-lines (one record per line). Each record carries the
//! surface form, lemma, POS, gender/number/case, and the Wiktionary
//! article title that supplied it (required for downstream CC BY-SA 4.0
//! attribution).
//!
//! References (verified):
//! - JSON-lines format: <https://jsonlines.org/>
//! - Attribution required by CC BY-SA 4.0:
//!   <https://creativecommons.org/licenses/by-sa/4.0/legalcode> § 3.a.

use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use serde::Serialize;

use de_morph::analysis::Case;
use de_morph::analysis::Gender;
use de_morph::analysis::Number;
use de_morph::analysis::UPOS;
use de_morph::analysis::Source;
use de_morph::wiktionary::ExtractedEntry;
use de_morph::wiktionary::dump::PageReader;
use de_morph::wiktionary::noun::extract_nouns;

const DEFAULT_INPUT: &str =
    "data/wiktionary/raw/dewiktionary-20260601-pages-articles.xml.bz2";
const DEFAULT_OUTPUT: &str = "data/wiktionary/processed/nouns.jsonl";
const PROGRESS_EVERY: u64 = 50_000;

#[derive(Serialize)]
struct OutputRecord<'a> {
    surface: &'a str,
    lemma: &'a str,
    pos: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    gender: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    number: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    case: Option<&'static str>,
    source: &'static str,
    source_title: &'a str,
}

fn entry_to_record(e: &ExtractedEntry) -> OutputRecord<'_> {
    OutputRecord {
        surface: &e.surface,
        lemma: &e.lemma,
        pos: pos_str(e.pos),
        gender: e.features.gender.map(gender_str),
        number: e.features.number.map(number_str),
        case: e.features.case.map(case_str),
        source: source_str(e.source),
        source_title: &e.source_title,
    }
}

fn source_str(s: Source) -> &'static str {
    match s {
        Source::Lexicon => "Lexicon",
        Source::Generated => "Generated",
        Source::Guessed => "Guessed",
    }
}

fn pos_str(p: UPOS) -> &'static str {
    match p {
        UPOS::NOUN => "Noun",
        UPOS::VERB => "Verb",
        UPOS::ADJ => "Adj",
        UPOS::ADV => "Adv",
        UPOS::PRON => "Pron",
        UPOS::DET => "Det",
        UPOS::NUM => "Num",
        UPOS::ADP => "Adp",
        UPOS::CCONJ => "Cconj",
        UPOS::SCONJ => "Sconj",
        UPOS::AUX => "Aux",
        UPOS::PART => "Part",
        UPOS::INTJ => "Intj",
        UPOS::PUNCT => "Punct",
        UPOS::SYM => "Sym",
        UPOS::X => "X",
        UPOS::PROPN => "Propn",
    }
}

fn gender_str(g: Gender) -> &'static str {
    match g {
        Gender::Masc => "Masc",
        Gender::Fem => "Fem",
        Gender::Neut => "Neut",
    }
}

fn number_str(n: Number) -> &'static str {
    match n {
        Number::Sg => "Sg",
        Number::Pl => "Pl",
    }
}

fn case_str(c: Case) -> &'static str {
    match c {
        Case::Nom => "Nom",
        Case::Gen => "Gen",
        Case::Dat => "Dat",
        Case::Acc => "Acc",
    }
}

struct Args {
    input: PathBuf,
    output: PathBuf,
    limit: Option<u64>,
}

fn parse_args() -> Result<Args> {
    let mut input = PathBuf::from(DEFAULT_INPUT);
    let mut output = PathBuf::from(DEFAULT_OUTPUT);
    let mut limit = None;

    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--input" => {
                i += 1;
                input = argv
                    .get(i)
                    .context("--input requires a value")?
                    .into();
            }
            "--output" => {
                i += 1;
                output = argv
                    .get(i)
                    .context("--output requires a value")?
                    .into();
            }
            "--limit" => {
                i += 1;
                limit = Some(
                    argv.get(i)
                        .context("--limit requires a value")?
                        .parse::<u64>()
                        .context("--limit must be an integer")?,
                );
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }
    Ok(Args {
        input,
        output,
        limit,
    })
}

fn print_usage() {
    eprintln!(
        "extract-nouns — extract German noun analyses from a Wiktionary dump\n\
\n\
Usage:\n\
  cargo run --release --features extractor --bin extract-nouns -- [options]\n\
\n\
Options:\n\
  --input <PATH>    Input .xml.bz2 dump file\n\
                    (default: {DEFAULT_INPUT})\n\
  --output <PATH>   Output .jsonl file\n\
                    (default: {DEFAULT_OUTPUT})\n\
  --limit <N>       Stop after processing N main-namespace pages\n\
                    (default: no limit)\n\
  --help            Show this message and exit."
    );
}

fn main() -> Result<()> {
    let args = parse_args()?;

    eprintln!("Reading {}", args.input.display());
    eprintln!("Writing {}", args.output.display());
    if let Some(n) = args.limit {
        eprintln!("Limit:   {n} main-namespace pages");
    }

    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent).context("creating output directory")?;
    }

    let out_file = File::create(&args.output).context("opening output file")?;
    let mut writer = BufWriter::with_capacity(1 << 20, out_file);

    let reader = PageReader::open_bz2(&args.input)
        .with_context(|| format!("opening dump at {}", args.input.display()))?;

    let start = Instant::now();
    let mut pages_total: u64 = 0;
    let mut pages_main: u64 = 0;
    let mut entries: u64 = 0;
    let mut errors: u64 = 0;

    for page in reader {
        let page = match page {
            Ok(p) => p,
            Err(e) => {
                eprintln!("xml read error: {e}");
                errors += 1;
                continue;
            }
        };
        pages_total += 1;

        if !page.is_main_namespace() {
            continue;
        }
        pages_main += 1;

        for entry in extract_nouns(&page.title, &page.text) {
            let record = entry_to_record(&entry);
            serde_json::to_writer(&mut writer, &record).context("write json")?;
            writer.write_all(b"\n").context("newline")?;
            entries += 1;
        }

        if pages_main % PROGRESS_EVERY == 0 {
            eprintln!(
                "  pages_total={pages_total} pages_main={pages_main} entries={entries} elapsed={:.1}s",
                start.elapsed().as_secs_f64()
            );
        }

        if let Some(limit) = args.limit {
            if pages_main >= limit {
                eprintln!("  --limit reached, stopping");
                break;
            }
        }
    }

    writer.flush().context("flushing output")?;

    let elapsed = start.elapsed().as_secs_f64();
    eprintln!("Done.");
    eprintln!("  pages_total = {pages_total}");
    eprintln!("  pages_main  = {pages_main}");
    eprintln!("  entries     = {entries}");
    eprintln!("  errors      = {errors}");
    eprintln!("  elapsed     = {elapsed:.1}s");
    eprintln!(
        "  rate        = {:.0} pages/s",
        pages_total as f64 / elapsed.max(0.001)
    );

    Ok(())
}
