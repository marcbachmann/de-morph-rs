//! Demonstrate the compound splitter on the live lexicon.
//!
//! Requires `data/lexicon/lexicon.{fst,dat}`. Run:
//!   `cargo run --release --features extractor --example compound_demo`

use de_morph::Lexicon;

const COMPOUNDS: &[&str] = &[
    "Lehrerzimmer",
    "Buchhandlung",
    "Wassermelone",
    "Bundestag", // expects linker -es-
    "Hausarbeit",
    "Schreibtischlampe", // 3-part: Schreib + Tisch + Lampe
    "Wörterbuch",
    "Donaudampfschiff", // expects 3 parts: Donau + Dampf + Schiff
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Loading lexicon...");
    let lex = Lexicon::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    eprintln!(
        "  {} surfaces, {} lemmas\n",
        lex.num_surfaces(),
        lex.num_lemmas()
    );

    for compound in COMPOUNDS {
        println!("=== {compound} ===");
        let ranked = lex.split_compound_ranked(compound);
        if ranked.is_empty() {
            println!("  (no valid splittings)");
        } else {
            // Deduplicate while preserving score-order.
            let mut seen: Vec<(Vec<String>, f64)> = Vec::new();
            for (parts, score) in ranked {
                if !seen.iter().any(|(p, _)| p == &parts) {
                    seen.push((parts, score));
                }
            }
            for (split, score) in seen.iter().take(5) {
                println!("  {:>6.2}  {}", score, split.join(" + "));
            }
            if seen.len() > 5 {
                println!("  ... ({} more)", seen.len() - 5);
            }
        }
        println!();
    }
    Ok(())
}
