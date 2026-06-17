use de_morph::Analyzer;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = Analyzer::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    for s in &["wer", "wessen", "wem", "wen", "was"] {
        let hits = a.analyze(s);
        println!("==== {s} ({} hits) ====", hits.len());
        for h in hits.iter().take(5) {
            print!("  {:<8} {:?}", h.lemma, h.pos);
            if let Some(c) = h.features.case { print!(" {:?}", c); }
            if let Some(n) = h.features.number { print!(" {:?}", n); }
            if let Some(g) = h.features.gender { print!(" {:?}", g); }
            println!("  source={:?}", h.source);
        }
    }
    Ok(())
}
