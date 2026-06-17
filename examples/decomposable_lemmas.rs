//! Count how many noun lemmas in the current lexicon are themselves
//! decomposable as compounds of OTHER lexicon entries. Useful for
//! gauging the potential size win of suppressing compound-noun
//! ingestion during the build.
//!
//! Methodology:
//! 1. Walk the noun JSONL files to enumerate every unique noun lemma.
//! 2. For each lemma ≥ 6 chars, ask `Lexicon::split_compound_detailed`
//!    if it has any valid decomposition.
//! 3. Bucket by part count and minimum-part length.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::Instant;

use de_morph::Lexicon;

const FST: &str = "data/lexicon/lexicon.fst";
const DAT: &str = "data/lexicon/lexicon.dat";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argv: Vec<String> = std::env::args().collect();
    let jsonl = argv
        .get(1)
        .cloned()
        .unwrap_or_else(|| "data/wiktionary/processed/nouns.jsonl".to_string());

    eprintln!("Loading lexicon...");
    let lex = Lexicon::open(FST, DAT)?;
    eprintln!("  {} surfaces, {} lemmas", lex.num_surfaces(), lex.num_lemmas());

    eprintln!("Enumerating unique noun lemmas from {jsonl}");
    let mut lemmas: HashSet<String> = HashSet::new();
    let file = File::open(&jsonl)?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if let Some(start) = line.find("\"lemma\":\"") {
            let rest = &line[start + 9..];
            if let Some(end) = rest.find('"') {
                lemmas.insert(rest[..end].to_string());
            }
        }
    }
    eprintln!("  {} unique noun lemmas", lemmas.len());

    let t = Instant::now();
    let mut total = 0u64;
    let mut decomposable = 0u64;
    let mut by_part_count: [u64; 8] = [0; 8];
    let mut by_min_len: [u64; 20] = [0; 20];
    let mut examples: Vec<(String, String)> = Vec::new();
    let mut idiomatic_examples: Vec<String> = Vec::new();

    for lemma in &lemmas {
        total += 1;
        // Compound splitter requires at least 6 chars (two ≥3-char parts).
        if lemma.chars().count() < 6 {
            continue;
        }
        let splits = lex.split_compound_detailed_ranked(lemma);
        if let Some((best, _)) = splits.first() {
            decomposable += 1;
            let np = best.parts.len().min(7);
            by_part_count[np] += 1;
            let min_len = best
                .parts
                .iter()
                .map(|p| p.chars().count())
                .min()
                .unwrap_or(0)
                .min(19);
            by_min_len[min_len] += 1;
            if examples.len() < 20 {
                examples.push((lemma.clone(), best.display()));
            }
        } else {
            // Lemma is 6+ chars but no compound decomposition found.
            // Likely a non-compound polysyllabic lemma (Universität,
            // Reaktion) — kept in lexicon as opaque entry.
            if idiomatic_examples.len() < 10
                && lemma.chars().count() > 8
                && lemma.chars().all(|c| c.is_alphabetic())
            {
                idiomatic_examples.push(lemma.clone());
            }
        }
    }

    let elapsed = t.elapsed().as_secs_f64();
    println!(
        "\n=== Decomposability analysis ({} lemmas, {:.1}s) ===",
        total, elapsed
    );
    println!(
        "  Decomposable as compound        : {} ({:.1}%)",
        decomposable,
        100.0 * decomposable as f64 / total as f64
    );
    println!(
        "  Non-decomposable (kept as opaque): {} ({:.1}%)",
        total - decomposable,
        100.0 * (total - decomposable) as f64 / total as f64
    );

    println!("\nBy part count of best decomposition:");
    for (n, &c) in by_part_count.iter().enumerate() {
        if c == 0 {
            continue;
        }
        println!("  {n}-part: {c:>10}");
    }

    println!("\nBy minimum part length of best decomposition:");
    for (n, &c) in by_min_len.iter().enumerate() {
        if c == 0 {
            continue;
        }
        println!("  min={n:>2} chars: {c:>10}");
    }

    println!("\nExamples of decomposable lemmas:");
    for (lemma, decomp) in &examples {
        println!("  {:<28} → {}", lemma, decomp);
    }

    println!("\nExamples of NON-decomposable long lemmas (must keep):");
    for lemma in &idiomatic_examples {
        println!("  {lemma}");
    }

    // Storage estimate: each ingested compound has ~8 inflections (Sg+Pl
    // × Nom/Gen/Dat/Acc), each costing one 8-byte AnalysisRecord plus a
    // share of the lemma string. ~10 bytes per record amortised.
    let bytes_saved = decomposable * 8 * 10;
    println!(
        "\nEstimated side-table savings if all decomposables were suppressed:"
    );
    println!(
        "  ~{:.1} MiB ({} lemmas × ~8 inflections × ~10 B/record amortised)",
        bytes_saved as f64 / (1024.0 * 1024.0),
        decomposable
    );
    println!(
        "  (Current side table is ~24.8 MiB; this would be a ~{:.0}% reduction)",
        100.0 * bytes_saved as f64 / (24.8 * 1024.0 * 1024.0)
    );

    Ok(())
}
