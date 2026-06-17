use de_morph::Analyzer;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = Analyzer::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    for s in &["bekannt", "bestellt", "Letzte", "letzt", "bestellten"] {
        let hits = a.analyze(s);
        print!("{:>12} ({}):", s, hits.len());
        for h in hits.iter().take(3) {
            print!(" [{}/{:?}]", h.lemma, h.pos);
        }
        println!();
    }
    Ok(())
}
