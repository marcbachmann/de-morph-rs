use de_morph::Lexicon;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lex = Lexicon::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    // Test cases: try some compositions that should NOT be allowed
    let invalid_tests = [
        // "Bunde + Stag" — Bunde is Dat Sg of Bund (not a lemma), Stag is rare
        (
            "Bundestag",
            "Bunde + Stag (invalid: Bunde is Dat Sg, not a base)",
        ),
        (
            "Hausarbeit",
            "Hau + Arbeit (invalid: Hau is interjection, not noun base)",
        ),
        (
            "Bundestag",
            "Bun + des + Tag (invalid: Bun is not a valid base)",
        ),
        (
            "Wörterbuch",
            "Wörter + Buch (valid: Wörter is Pl form of Wort)",
        ),
        (
            "Tageszeitung",
            "Tag + es + Zeitung (valid: -es- linker after monosyllabic neut Masc/Neut)",
        ),
        (
            "Sonnenstrahl",
            "Sonne + n + Strahl (valid: -n- after fem -e)",
        ),
    ];
    for (compound, expected) in &invalid_tests {
        println!("=== {compound} ({expected}) ===");
        let splits = lex.split_compound_detailed_ranked(compound);
        for (split, score) in splits.iter().take(5) {
            let reassembled = split.reassemble();
            let ok = if reassembled == *compound {
                "✓"
            } else {
                "✗"
            };
            println!(
                "  {:>6.2}  {:<35}  →  {} {}",
                score,
                split.display(),
                reassembled,
                ok
            );
        }
        // Also probe individual parts
        let key_words = match *compound {
            "Bundestag" => &["Bund", "Bunde", "Bundes", "Tag", "Stag"][..],
            "Hausarbeit" => &["Haus", "Hau", "Arbeit"][..],
            "Wörterbuch" => &["Wörter", "Wort", "Buch"][..],
            "Tageszeitung" => &["Tag", "Tages", "Zeitung"][..],
            "Sonnenstrahl" => &["Sonne", "Sonnen", "Strahl"][..],
            _ => &[][..],
        };
        if !key_words.is_empty() {
            println!("  parts in lexicon:");
            for w in key_words {
                let hits = lex.analyze(w);
                if hits.is_empty() {
                    println!("    {w}: (not in lexicon)");
                } else {
                    let summary = hits
                        .iter()
                        .take(3)
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
                        .collect::<Vec<_>>()
                        .join("; ");
                    println!("    {w}: {summary}");
                }
            }
        }
        println!();
    }
    Ok(())
}
