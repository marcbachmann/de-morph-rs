use de_morph::Lexicon;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lex = Lexicon::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    println!("=== split_compound_detailed('Hausarbeit') — raw output ===");
    for (i, split) in lex.split_compound_detailed("Hausarbeit").iter().enumerate() {
        let parts = &split.parts;
        println!(
            "  [{i}] {} chars total: {:?}  linkers={:?}",
            parts.iter().map(|s| s.chars().count()).sum::<usize>(),
            parts,
            split.linkers
        );
        // Reassemble and check
        println!("       reassemble = {:?}", split.reassemble());
    }
    // Direct linker check
    println!("\n=== is_valid_compound_linker('Hau', 's') = ? ===");
    let haus = lex.analyze("Haus");
    println!("  analyze('Haus') = {} analyses", haus.len());
    for a in &haus {
        println!("    lemma={:?} pos={:?}", a.lemma, a.pos);
    }
    let has_hau_form = haus
        .iter()
        .any(|a| a.lemma == "Hau" && a.pos == de_morph::analysis::UPOS::NOUN);
    println!("  any lemma='Hau' AND pos=Noun: {has_hau_form}");
    Ok(())
}
