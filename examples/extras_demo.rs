//! Demo of features-beyond-the-paradigm: vowel reduction, ordinals,
//! compound numerals, and PronType assignments. Runs a curated
//! showcase of sample groups. For analysing an arbitrary sentence or
//! word list, use `analyze_demo` instead.

use de_morph::Analyzer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = Analyzer::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    let samples = [
        ("VOWEL-REDUCTION", &["eure", "eures", "unsre", "unseres"][..]),
        ("ORDINALS",        &["erste", "ersten", "zweite", "dritte", "zehnte", "zwanzigste", "hundertste"][..]),
        ("COMPOUND-NUMS",   &["einundzwanzig", "fünfundsiebzig", "neunundneunzig", "zweihundert", "dreitausend"][..]),
        ("PRONTYPE-PRS",    &["ich", "mein"][..]),
        ("PRONTYPE-REFL",   &["sich"][..]),
        ("PRONTYPE-ART",    &["der", "ein"][..]),
        ("PRONTYPE-INT",    &["wer", "was"][..]),
        ("PRONTYPE-IND",    &["jemand", "etwas"][..]),
    ];
    for (label, words) in samples {
        println!("--- {label} ---");
        for s in words {
            let hits = a.analyze(s);
            print!("  {:<16} (n={}) ", s, hits.len());
            for h in hits.iter().take(2) {
                print!("[{:?}/{}", h.pos, h.lemma);
                if let Some(pt) = h.features.pron_type { print!(" pt={pt:?}"); }
                if let Some(p) = h.features.poss_person { print!(" pp={p:?}"); }
                if let Some(n) = h.features.poss_number { print!(" pn={n:?}"); }
                if let Some(c) = h.features.case { print!(" {c:?}"); }
                print!("] ");
            }
            println!();
        }
    }
    Ok(())
}
