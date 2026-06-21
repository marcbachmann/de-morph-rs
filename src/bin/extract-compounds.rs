//! Extract Wiktionary-declared compounds from the dump.
//!
//! Reads `{{Herkunft}}` sections and emits one JSONL record per
//! compound declaration with the constituent parts, optional
//! Fugenelement, and compound type (Determinativ / Possessiv /
//! Kopulativ / Other).
//!
//! Output schema:
//!
//! ```json
//! {
//!   "lemma": "Wörterbuch",
//!   "compound_type": "Determinativ",
//!   "parts": ["Wort", "Buch"],
//!   "fugenelement": "er",
//!   "source_title": "Wörterbuch"
//! }
//! ```

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use de_morph::wiktionary::compound::extract_compounds;
use de_morph::wiktionary::dump::PageReader;

const DEFAULT_INPUT: &str = "data/wiktionary/raw/dewiktionary-20260601-pages-articles.xml.bz2";
const DEFAULT_OUTPUT: &str = "data/wiktionary/processed/compounds.jsonl";
const PROGRESS_EVERY: u64 = 200_000;

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
                eprintln!("extract-compounds [--input PATH] [--output PATH]");
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
    let mut compounds: u64 = 0;

    for page in reader {
        let page = page?;
        pages_total += 1;
        if !page.is_main_namespace() {
            continue;
        }
        pages_main += 1;
        for entry in extract_compounds(&page.title, &page.text) {
            serde_json::to_writer(&mut writer, &entry)?;
            writer.write_all(b"\n")?;
            compounds += 1;
        }
        if pages_main % PROGRESS_EVERY == 0 {
            eprintln!(
                "  pages_main={pages_main} compounds={compounds} elapsed={:.1}s",
                start.elapsed().as_secs_f64()
            );
        }
    }

    writer.flush()?;
    let elapsed = start.elapsed().as_secs_f64();
    eprintln!("Done.");
    eprintln!("  pages_total = {pages_total}");
    eprintln!("  pages_main  = {pages_main}");
    eprintln!("  compounds   = {compounds}");
    eprintln!("  elapsed     = {elapsed:.1}s");
    Ok(())
}
