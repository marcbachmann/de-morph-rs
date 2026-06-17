use de_morph::Analyzer;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = Analyzer::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    let samples = [
        ("REFLEXIVES", &["sich", "mich"][..]),
        ("RELATIVES",  &["dessen", "deren"][..]),
        ("INDEFINITES", &["jemand", "jemandem", "etwas", "nichts", "man", "alle", "viele"][..]),
        ("NUMERALS",   &["null", "drei", "zwanzig", "hundert", "tausend"][..]),
        ("CCONJ",      &["und", "oder", "aber"][..]),
        ("SCONJ",      &["dass", "weil", "wenn", "obwohl"][..]),
        ("ADP",        &["in", "auf", "mit", "von", "während", "trotz"][..]),
    ];
    for (label, words) in samples {
        println!("--- {label} ---");
        for s in words {
            let hits = a.analyze(s);
            let preview = hits.iter().take(2).map(|h| {
                let mut s = format!("[{:?}/{}", h.pos, h.lemma);
                if let Some(c) = h.features.case { s.push_str(&format!(" {:?}", c)); }
                if let Some(n) = h.features.number { s.push_str(&format!(" {:?}", n)); }
                s.push(']');
                s
            }).collect::<Vec<_>>().join(" ");
            println!("  {:<12} ({} hits) {preview}", s, hits.len());
        }
    }
    Ok(())
}
