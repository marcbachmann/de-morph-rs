use de_morph::Analyzer;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = Analyzer::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    for surface in &[
        "Tisch",
        "Tischen",
        "Bücher",
        "liebte",
        "großen",
        "ich",
        "der",
        "einundzwanzig",
        "erste",
    ] {
        let hits = a.analyze(surface);
        println!(
            "{surface:>15} ({} hits)  e.g. lemma={:?} features={:?}",
            hits.len(),
            hits.first().map(|h| &*h.lemma),
            hits.first().map(|h| h.features)
        );
    }
    Ok(())
}
