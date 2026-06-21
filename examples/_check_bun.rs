use de_morph::Lexicon;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lex = Lexicon::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    for w in &[
        "Bun", "Des", "des", "Bundes", "Bunde", "Hau", "Haus", "estag", "destag",
    ] {
        let hits = lex.analyze(w);
        let summary: Vec<String> = hits
            .iter()
            .take(5)
            .map(|h| {
                let mut s = format!("{}/{:?}", h.lemma, h.pos);
                if let Some(c) = h.features.case {
                    s.push_str(&format!(" {:?}", c));
                }
                if let Some(n) = h.features.number {
                    s.push_str(&format!(" {:?}", n));
                }
                s
            })
            .collect();
        println!("  {:>10}: {} hits — {}", w, hits.len(), summary.join("; "));
    }
    Ok(())
}
