//! Extract German proper nouns (Toponym, Vorname, Nachname, Eigenname,
//! Göttername, Bauwerksname) from a Wiktionary dump.
//!
//! Emits one JSONL record per case/number cell with `pos = "Propn"`,
//! lemma = page title. Place / surname / building-name entries get the
//! canonical 4-cell paradigm (Nom/Dat/Acc/Gen Sg); given names with a
//! structured `Deutsch Vorname Übersicht` template get whatever cells
//! the template provides.

use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use serde::Serialize;

use de_morph::analysis::{Case, Gender, Number, UPOS};
use de_morph::wiktionary::ExtractedEntry;
use de_morph::wiktionary::dump::PageReader;
use de_morph::wiktionary::propn::extract_proper_nouns;

const DEFAULT_INPUT: &str =
    "data/wiktionary/raw/dewiktionary-20260601-pages-articles.xml.bz2";
const DEFAULT_OUTPUT: &str = "data/wiktionary/processed/propn.jsonl";
const PROGRESS_EVERY: u64 = 200_000;

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

fn pos_str(p: UPOS) -> &'static str {
    match p {
        UPOS::PROPN => "Propn",
        _ => "Noun", // defensive — extract_proper_nouns should only emit Propn
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

fn entry_to_record(e: &ExtractedEntry) -> OutputRecord<'_> {
    OutputRecord {
        surface: &e.surface,
        lemma: &e.lemma,
        pos: pos_str(e.pos),
        gender: e.features.gender.map(gender_str),
        number: e.features.number.map(number_str),
        case: e.features.case.map(case_str),
        source: "Lexicon",
        source_title: &e.source_title,
    }
}

fn main() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let mut input = PathBuf::from(DEFAULT_INPUT);
    let mut output = PathBuf::from(DEFAULT_OUTPUT);
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--input" => {
                i += 1;
                input = argv.get(i).context("--input requires a value")?.into();
            }
            "--output" => {
                i += 1;
                output = argv.get(i).context("--output requires a value")?.into();
            }
            "--help" | "-h" => {
                eprintln!("extract-propn [--input PATH] [--output PATH]");
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    eprintln!("Reading {}", input.display());
    eprintln!("Writing {}", output.display());

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let out_file = File::create(&output)?;
    let mut writer = BufWriter::with_capacity(1 << 20, out_file);
    let reader = PageReader::open_bz2(&input)?;
    let start = Instant::now();
    let mut pages_total: u64 = 0;
    let mut pages_main: u64 = 0;
    let mut entries: u64 = 0;

    for page in reader {
        let page = page?;
        pages_total += 1;
        if !page.is_main_namespace() {
            continue;
        }
        pages_main += 1;
        for entry in extract_proper_nouns(&page.title, &page.text) {
            let rec = entry_to_record(&entry);
            serde_json::to_writer(&mut writer, &rec)?;
            writer.write_all(b"\n")?;
            entries += 1;
        }
        if pages_main % PROGRESS_EVERY == 0 {
            eprintln!(
                "  pages_main={pages_main} entries={entries} elapsed={:.1}s",
                start.elapsed().as_secs_f64()
            );
        }
    }

    writer.flush()?;
    let elapsed = start.elapsed().as_secs_f64();
    eprintln!("Done.");
    eprintln!("  pages_total = {pages_total}");
    eprintln!("  pages_main  = {pages_main}");
    eprintln!("  entries     = {entries}");
    eprintln!("  elapsed     = {elapsed:.1}s");
    Ok(())
}
