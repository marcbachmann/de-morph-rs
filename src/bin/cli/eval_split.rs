//! Validate the heuristic compound splitter against Wiktionary's
//! curated decomposition.
//!
//! For each entry in compounds.jsonl, we:
//!   1. Take the lemma (e.g. "Wörterbuch")
//!   2. Take Wiktionary's parts (e.g. ["Wort", "Buch"])
//!   3. Run `lex.split_compound_detailed_ranked(lemma)` and inspect
//!      the top-1 + top-k candidates
//!   4. Report match rates: top-1 exact, top-1 part-set, top-5 any-match
//!
//! We also track WHICH compounds the splitter completely misses (no
//! split returned) — those are the most informative for understanding
//! where rule limits hit.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::Instant;

use de_morph::{Lexicon, UPOS};

const COMPOUNDS: &str = "data/wiktionary/processed/compounds.jsonl";

#[derive(Default, Debug)]
struct Tally {
    total: u64,
    splitter_no_split: u64,
    top1_parts_set_matches: u64,
    top1_parts_order_matches: u64,
    top5_parts_set_matches: u64,
    fugenelement_match: u64,
    fugenelement_total: u64, // denominator only counts compounds with a Wiktionary-recorded Fugenelement
    // Lemma-normalized, noise-filtered scoring (true boundary accuracy).
    noise_skipped: u64,
    gloss_skipped: u64,
    clean_total: u64,
    clean_no_split: u64,
    clean_lemma_top1: u64,
    clean_lemma_top5: u64,
}

/// Tokens that appear as "parts" in compounds.jsonl but are extraction
/// artifacts, not morphemes: POS labels, grammatical-feature words,
/// grammar-process labels, and wiki-namespace markup. Stored lowercased;
/// matched case-insensitively. A gold record containing any of these is
/// dropped from the clean denominator.
const NOISE_PARTS: &[&str] = &[
    // POS labels
    "nomen",
    "substantiv",
    "präposition",
    "adjektiv",
    "verb",
    "adverb",
    "artikel",
    "pronomen",
    "numerale",
    "konjunktion",
    "partikel",
    "interjektion",
    "zahlwort",
    // grammatical-feature / register words
    "feminin",
    "maskulin",
    "neutrum",
    "femininum",
    "maskulinum",
    "singular",
    "plural",
    "rechtssprache",
    // grammar-process labels (deverbal/derivation terms, never used as
    // a compound constituent in this data)
    "substantiviert",
    "substantiven",
    "substantivierung",
    "verbstamm",
    "konversion",
];

fn is_noise_part(p: &str) -> bool {
    // Real morphemes never contain ':' (namespace markup), '.' (`subst.`),
    // or whitespace (`Gebundenes Lexem`).
    p.is_empty()
        || p.contains(':')
        || p.contains('.')
        || p.chars().any(char::is_whitespace)
        || NOISE_PARTS.contains(&p.to_lowercase().as_str())
}

/// A leading-hyphen token (`-er`, `-n`, `-s`) is a Fugenelement listed
/// as a part, not a content morpheme. Trailing-hyphen tokens
/// (`Untersee-`, a suspension) are real content and kept.
fn is_linker_token(p: &str) -> bool {
    p.starts_with('-')
}

/// Normalize a Wiktionary content part for comparison: drop a trailing
/// suspension hyphen, lowercase.
fn norm_wikt(p: &str) -> String {
    p.trim_end_matches('-').to_lowercase()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn lowercase_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Candidate lemma forms (lowercased) for a splitter-produced surface
/// segment. Includes the segment itself (covers pure case differences
/// like `Frei`/`frei`), the lemmas of its direct/capitalized/lowercased
/// analyses (covers inflection + Fugen: `Wörter`→`Wort`, `Geistes`→
/// `Geist`), and the verb infinitive when the segment is a verb stem
/// (`Klär`→`klären`, `Leucht`→`leuchten`).
fn part_lemmas(lex: &Lexicon, part: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    out.insert(part.to_lowercase());
    for form in [part.to_string(), capitalize(part), lowercase_first(part)] {
        for a in lex.analyze(&form) {
            out.insert(a.lemma.to_lowercase());
        }
    }
    let lower = lowercase_first(part);
    for inf in [format!("{lower}en"), format!("{lower}n")] {
        if lex.is_lemma_of_pos(&inf, UPOS::VERB) {
            out.insert(inf.to_lowercase());
        }
    }
    out
}

/// True iff `parts` reproduce `wikt_content` (already normalized) after
/// lemma-normalization — order-insensitive, with matching cardinality
/// (so over-/under-splits like `Untersee-Boot`→`Unter+see+Boot` still
/// count as errors).
fn lemma_set_match(lex: &Lexicon, parts: &[String], wikt_content: &[String]) -> bool {
    if parts.len() != wikt_content.len() {
        return false;
    }
    let mut used = vec![false; parts.len()];
    for w in wikt_content {
        let mut found = false;
        for (i, p) in parts.iter().enumerate() {
            if !used[i] && part_lemmas(lex, p).contains(w) {
                used[i] = true;
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

#[derive(Debug)]
struct CompoundRecord {
    lemma: String,
    parts: Vec<String>,
    fugenelement: Option<String>,
    compound_type: String,
}

fn parse_jsonl_line(line: &str) -> Option<CompoundRecord> {
    // Minimal JSON parsing — avoids pulling in serde just for the
    // diagnostic.
    let lemma = pull_string_field(line, "\"lemma\":\"")?;
    let parts = pull_string_array(line, "\"parts\":[")?;
    let fugenelement = pull_optional_string(line, "\"fugenelement\":");
    let compound_type = pull_string_field(line, "\"compound_type\":\"")
        .unwrap_or_else(|| "Determinativ".to_string());
    Some(CompoundRecord {
        lemma,
        parts,
        fugenelement,
        compound_type,
    })
}

fn pull_string_field(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn pull_string_array(line: &str, key: &str) -> Option<Vec<String>> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    let end = rest.find(']')?;
    let array_body = &rest[..end];
    let mut out = Vec::new();
    let mut chars = array_body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut s = String::new();
            for ch in chars.by_ref() {
                if ch == '"' {
                    break;
                }
                s.push(ch);
            }
            out.push(s);
        }
    }
    Some(out)
}

fn pull_optional_string(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    if rest.starts_with("null") {
        return None;
    }
    if !rest.starts_with('"') {
        return None;
    }
    let after_quote = &rest[1..];
    let end = after_quote.find('"')?;
    Some(after_quote[..end].to_string())
}

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let compounds_path = args.first().map(String::as_str).unwrap_or(COMPOUNDS);
    let lex = crate::loader::lexicon()?;
    eprintln!(
        "Lexicon: {} lemmas, {} surfaces",
        lex.num_lemmas(),
        lex.num_surfaces()
    );
    eprintln!("Reading {compounds_path}");
    let file = File::open(compounds_path)?;
    let reader = BufReader::new(file);

    let t = Instant::now();
    let mut tally = Tally::default();
    let mut sample_no_split: Vec<String> = Vec::new();
    let mut sample_disagreement: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
    let mut sample_lemma_miss: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let Some(rec) = parse_jsonl_line(&line) else {
            continue;
        };
        // Only evaluate against Determinativ compounds; the others are
        // too few and have idiosyncratic structures (Possessivkompositum
        // attaches a suffix like -chen rather than another noun).
        if rec.compound_type != "Determinativ" {
            continue;
        }
        // Need ≥ 2 parts for a sensible comparison.
        if rec.parts.len() < 2 {
            continue;
        }
        tally.total += 1;

        let splits = lex.split_compound_detailed_ranked(&rec.lemma);

        // --- Lemma-normalized, noise-filtered scoring (true boundary
        // accuracy). Runs for every record incl. no-split ones, so it
        // sits before the early `continue` below. ---
        {
            let noisy = rec.parts.iter().any(|p| is_noise_part(p));
            let wikt_content: Vec<String> = rec
                .parts
                .iter()
                .filter(|p| !is_linker_token(p))
                .map(|p| norm_wikt(p))
                .filter(|p| !p.is_empty())
                .collect();
            // Gloss contamination: a Determinativ compound is
            // concatenative, so the summed length of its real parts is
            // ~≤ the lemma length. Definition words that leaked in as
            // extra "parts" (betrunken, schienenstrang) push the sum well
            // past it. Slack of 4 absorbs e-elision (Erdbeere→Erdbeer)
            // and lemma-vs-surface length wobble.
            let content_len: usize = wikt_content.iter().map(|p| p.chars().count()).sum();
            let gloss = content_len > rec.lemma.chars().count() + 4;
            if noisy {
                tally.noise_skipped += 1;
            } else if gloss {
                tally.gloss_skipped += 1;
            }
            if !noisy && !gloss && wikt_content.len() >= 2 {
                tally.clean_total += 1;
                if splits.is_empty() {
                    tally.clean_no_split += 1;
                } else {
                    if lemma_set_match(&lex, &splits[0].0.parts, &wikt_content) {
                        tally.clean_lemma_top1 += 1;
                    } else if sample_lemma_miss.len() < 15 {
                        sample_lemma_miss.push((
                            rec.lemma.clone(),
                            wikt_content.clone(),
                            splits[0].0.parts.clone(),
                        ));
                    }
                    if splits
                        .iter()
                        .take(5)
                        .any(|(s, _)| lemma_set_match(&lex, &s.parts, &wikt_content))
                    {
                        tally.clean_lemma_top5 += 1;
                    }
                }
            }
        }

        if splits.is_empty() {
            tally.splitter_no_split += 1;
            if sample_no_split.len() < 20 {
                sample_no_split.push(format!("{} ({})", rec.lemma, rec.parts.join("+")));
            }
            continue;
        }
        let wikt_set: HashSet<String> = rec.parts.iter().cloned().collect();

        // Top-1 matching.
        let top = &splits[0].0;
        let top_set: HashSet<String> = top.parts.iter().cloned().collect();
        let order_eq = top.parts == rec.parts;
        let set_eq = top_set == wikt_set;
        if order_eq {
            tally.top1_parts_order_matches += 1;
        }
        if set_eq {
            tally.top1_parts_set_matches += 1;
        }
        if !order_eq && sample_disagreement.len() < 20 {
            sample_disagreement.push((rec.lemma.clone(), rec.parts.clone(), top.parts.clone()));
        }

        // Top-5 set match.
        let any_top5_set = splits.iter().take(5).any(|(s, _)| {
            let set: HashSet<String> = s.parts.iter().cloned().collect();
            set == wikt_set
        });
        if any_top5_set {
            tally.top5_parts_set_matches += 1;
        }

        // Fugenelement comparison (only count when Wiktionary specified one).
        if let Some(wikt_fuge) = &rec.fugenelement {
            tally.fugenelement_total += 1;
            // Find the first split with parts matching Wiktionary, then
            // compare its linker.
            let matching = splits.iter().find(|(s, _)| {
                let set: HashSet<String> = s.parts.iter().cloned().collect();
                set == wikt_set
            });
            if let Some((s, _)) = matching {
                // Wiktionary fugenelement strings have leading "-" stripped
                // already (`er` not `-er`). Compare to first linker.
                let our_fuge = s.linkers.first().cloned().unwrap_or_default();
                if our_fuge == *wikt_fuge {
                    tally.fugenelement_match += 1;
                }
            }
        }
    }

    let elapsed = t.elapsed().as_secs_f64();
    eprintln!("Compared {} compounds in {:.1}s", tally.total, elapsed);

    let pct = |n: u64, d: u64| {
        if d == 0 {
            0.0
        } else {
            100.0 * n as f64 / d as f64
        }
    };

    println!("\n=== Splitter vs. Wiktionary ground truth ===");
    println!("  Compounds evaluated:               {}", tally.total);
    println!(
        "  No split returned (splitter miss): {} ({:.1}%)",
        tally.splitter_no_split,
        pct(tally.splitter_no_split, tally.total)
    );

    let with_split = tally.total - tally.splitter_no_split;
    println!("\n  Of the {} compounds the splitter handled:", with_split);
    println!(
        "  Top-1 parts (exact order):         {} ({:.1}%)",
        tally.top1_parts_order_matches,
        pct(tally.top1_parts_order_matches, with_split)
    );
    println!(
        "  Top-1 parts (any order):           {} ({:.1}%)",
        tally.top1_parts_set_matches,
        pct(tally.top1_parts_set_matches, with_split)
    );
    println!(
        "  Top-5 parts (any-of-top-5 match):  {} ({:.1}%)",
        tally.top5_parts_set_matches,
        pct(tally.top5_parts_set_matches, with_split)
    );

    println!("\n  Fugenelement agreement (only counted when Wiktionary explicitly annotates one):");
    println!(
        "    Linker match: {}/{} ({:.1}%)",
        tally.fugenelement_match,
        tally.fugenelement_total,
        pct(tally.fugenelement_match, tally.fugenelement_total)
    );

    println!("\n=== Lemma-normalized + noise-filtered (true boundary accuracy) ===");
    println!(
        "  Noisy gold records skipped (label):{}",
        tally.noise_skipped
    );
    println!(
        "  Gold records skipped (gloss leak): {}",
        tally.gloss_skipped
    );
    println!("  Clean compounds evaluated:         {}", tally.clean_total);
    let clean_handled = tally.clean_total - tally.clean_no_split;
    println!(
        "  No split returned (clean):         {} ({:.1}%)",
        tally.clean_no_split,
        pct(tally.clean_no_split, tally.clean_total)
    );
    println!("\n  Of the {clean_handled} clean compounds the splitter handled:");
    println!(
        "  Top-1 lemma match (boundary OK):   {} ({:.1}%)",
        tally.clean_lemma_top1,
        pct(tally.clean_lemma_top1, clean_handled)
    );
    println!(
        "  Top-5 lemma match:                 {} ({:.1}%)",
        tally.clean_lemma_top5,
        pct(tally.clean_lemma_top5, clean_handled)
    );

    println!("\nExamples — splitter returned no split:");
    for s in &sample_no_split[..sample_no_split.len().min(10)] {
        println!("  {s}");
    }

    println!("\nExamples — top-1 surface disagreement (Wiktionary truth vs. our top pick):");
    for (lemma, wikt, ours) in &sample_disagreement[..sample_disagreement.len().min(10)] {
        println!("  {lemma}:  Wikt={:?} vs ours={:?}", wikt, ours);
    }

    println!("\nExamples — genuine boundary errors (wrong even after lemma-normalization):");
    for (lemma, wikt, ours) in &sample_lemma_miss[..sample_lemma_miss.len().min(12)] {
        println!("  {lemma}:  wikt={:?} vs ours={:?}", wikt, ours);
    }

    Ok(())
}
