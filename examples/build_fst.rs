//! Build an FST from the noun-extraction JSONL and report its size.
//!
//! Reads `data/wiktionary/processed/nouns.jsonl` (must already exist —
//! produced by `cargo run --release --features extractor --bin extract-nouns`)
//! and writes two artefacts side by side:
//!
//!   - `nouns.surfaces.fst` — `fst::Set` of unique surface forms.
//!     Compact lower-bound size for the lookup structure only; no
//!     per-form payload.
//!   - `nouns.map.fst`      — `fst::Map<surface, packed-u64>`. The u64
//!     packs `(analysis_count << 32) | side_table_offset`. A separate
//!     side table would store the actual analyses; here we only build
//!     the map structure and report its size, plus the size of the
//!     side-table payload that would be needed.
//!
//! Run: `cargo run --release --example build_fst`

use std::collections::BTreeMap;
use std::fs::{metadata, File};
use std::io::{BufRead, BufReader, BufWriter};
use std::time::Instant;

use anyhow::{Context, Result};
use fst::{MapBuilder, SetBuilder};
use serde::Deserialize;

const JSONL_FILES: &[&str] = &[
    "data/wiktionary/processed/nouns.jsonl",
    "data/wiktionary/processed/verbs.jsonl",
];
const OUT_SET: &str = "data/wiktionary/processed/lexicon.surfaces.fst";
const OUT_MAP: &str = "data/wiktionary/processed/lexicon.map.fst";

#[derive(Deserialize)]
struct Record {
    surface: String,
    lemma: String,
    #[allow(dead_code)]
    pos: String,
}

fn main() -> Result<()> {
    let start = Instant::now();

    // Read all records from every input JSONL, grouped by surface.
    let mut by_surface: BTreeMap<String, Vec<Record>> = BTreeMap::new();
    let mut total_records: u64 = 0;
    let mut input_bytes: u64 = 0;
    for path in JSONL_FILES {
        eprintln!("Reading {path}");
        let meta = metadata(path).with_context(|| format!("stat {path}"))?;
        input_bytes += meta.len();
        let file = File::open(path).with_context(|| format!("opening {path}"))?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            let rec: Record = serde_json::from_str(&line).context("parse JSONL line")?;
            total_records += 1;
            by_surface.entry(rec.surface.clone()).or_default().push(rec);
        }
    }
    eprintln!(
        "  total {total_records} records across {} files, {} unique surfaces ({:.1}s)",
        JSONL_FILES.len(),
        by_surface.len(),
        start.elapsed().as_secs_f64()
    );

    // (1) Surface-only Set
    eprintln!("Building Set FST at {OUT_SET}");
    let set_out = File::create(OUT_SET)?;
    let mut set_builder = SetBuilder::new(BufWriter::new(set_out))?;
    for surface in by_surface.keys() {
        set_builder.insert(surface)?;
    }
    set_builder.finish()?;

    // (2) Map FST: surface -> packed(count << 32 | side_table_offset)
    // We don't actually write the side table; we only compute its size.
    eprintln!("Building Map FST at {OUT_MAP}");
    let map_out = File::create(OUT_MAP)?;
    let mut map_builder = MapBuilder::new(BufWriter::new(map_out))?;
    // Each Analysis as a side-table row: lemma (interned ID, 4B) +
    // PackedFeatures (4B) + UPOS (1B) + Source (1B) = 10B padded to 12B.
    const ROW_BYTES: u64 = 12;
    let mut side_offset: u64 = 0;
    let mut side_table_bytes: u64 = 0;
    for (surface, recs) in &by_surface {
        let count = recs.len() as u64;
        let packed = (count << 32) | side_offset;
        map_builder.insert(surface, packed)?;
        side_offset += count * ROW_BYTES;
        side_table_bytes += count * ROW_BYTES;
    }
    map_builder.finish()?;

    // Estimate the lemma interning table size (unique lemmas × average
    // UTF-8 length + a u32 offset table).
    let unique_lemmas: BTreeMap<&str, ()> = by_surface
        .values()
        .flat_map(|v| v.iter().map(|r| (r.lemma.as_str(), ())))
        .collect();
    let lemma_bytes: u64 = unique_lemmas.keys().map(|s| s.len() as u64).sum::<u64>()
        + (unique_lemmas.len() as u64) * 4;

    // Report sizes.
    let set_size = metadata(OUT_SET)?.len();
    let map_size = metadata(OUT_MAP)?.len();

    eprintln!("\n=== Sizes ===");
    eprintln!(
        "  Records (JSONL input total across {} files):              {:>10}",
        JSONL_FILES.len(),
        humanize(input_bytes)
    );
    eprintln!("  Total records:                                            {total_records:>10}");
    eprintln!(
        "  Unique surfaces:                                          {:>10}",
        by_surface.len()
    );
    eprintln!(
        "  Unique lemmas:                                            {:>10}",
        unique_lemmas.len()
    );
    eprintln!();
    eprintln!(
        "  (1) Surface Set FST (lookup only, no payload):            {:>10}",
        humanize(set_size)
    );
    eprintln!(
        "  (2) Surface-to-payload-pointer Map FST:                   {:>10}",
        humanize(map_size)
    );
    eprintln!(
        "  (2a) Side table needed (analyses, 12 B / row):            {:>10}",
        humanize(side_table_bytes)
    );
    eprintln!(
        "  (2b) Lemma intern table (unique lemmas + u32 offsets):    {:>10}",
        humanize(lemma_bytes)
    );
    eprintln!();
    eprintln!(
        "  Estimated total nouns-only runtime artefact:              {:>10}",
        humanize(map_size + side_table_bytes + lemma_bytes)
    );
    eprintln!();
    eprintln!("  Elapsed: {:.1}s", start.elapsed().as_secs_f64());

    Ok(())
}

fn humanize(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {} ({bytes} bytes)", UNITS[unit])
    }
}
