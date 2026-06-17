use de_morph::Analyzer;

const SAMPLES: &[&str] = &[
    // Personal pronouns
    "mich", "mir",
    "er", "ihn",
    "sie", "ihr",
    // Articles
    "der", "die", "das",
    "ein", "einer",
    "kein", "keine",
    // Possessives (new)
    "mein", "meinen", "meinem",
    "unser", "unsere",
    // Demonstratives (new)
    "dieser", "diese", "diesem",
    "welcher", "welches",
    "jedem",
    // Adverbs
    "heute", "schon",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let analyzer = Analyzer::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    for surface in SAMPLES {
        let hits = analyzer.analyze(surface);
        println!("==== {surface} ({} hits) ====", hits.len());
        for a in hits.iter().take(4) {
            print!("  {:<10} {:?}", a.lemma, a.pos);
            if let Some(p) = a.features.person { print!(" P{:?}", p); }
            if let Some(n) = a.features.number { print!(" {:?}", n); }
            if let Some(g) = a.features.gender { print!(" {:?}", g); }
            if let Some(c) = a.features.case { print!(" {:?}", c); }
            println!("  source={:?}", a.source);
        }
        if hits.len() > 4 {
            println!("  ... ({} more)", hits.len() - 4);
        }
    }
    Ok(())
}
