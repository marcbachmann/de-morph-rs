//! `de-morph split` — show ranked compound splittings.
//!
//! With no arguments, runs a built-in sample set; otherwise splits each
//! word passed on the command line:
//!   de-morph split Lehrerzimmer Donaudampfschiff

const SAMPLES: &[&str] = &[
    "Lehrerzimmer",
    "Buchhandlung",
    "Wassermelone",
    "Bundestag", // expects linker -es-
    "Hausarbeit",
    "Schreibtischlampe", // 3-part: Schreib + Tisch + Lampe
    "Wörterbuch",
    "Donaudampfschiff", // expects 3 parts: Donau + Dampf + Schiff
];

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Loading lexicon...");
    let lex = crate::loader::lexicon()?;
    eprintln!(
        "  {} surfaces, {} lemmas\n",
        lex.num_surfaces(),
        lex.num_lemmas()
    );

    let words: Vec<&str> = if args.is_empty() {
        SAMPLES.to_vec()
    } else {
        args.iter().map(String::as_str).collect()
    };

    for compound in words {
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
