//! Demonstrate noun-paradigm generation and OOV dative prediction.
//!
//! Run with: `cargo run --example dative_demo`

use de_morph::analysis::Gender;
use de_morph::paradigm::noun::{
    generate_noun_paradigm, guess_noun, predict_dative_forms, NounClass,
};

fn main() {
    println!("=== Generation: Tisch (strong masc), all 8 cells ===");
    for (surface, a) in
        generate_noun_paradigm("Tisch", Gender::Masc, NounClass::Strong, Some("Tische"))
    {
        println!(
            "  {:>10}  {:?} {:?}  source={:?}",
            surface,
            a.features.case.unwrap(),
            a.features.number.unwrap(),
            a.source
        );
    }

    println!("\n=== Generation: Bauer (weak masc) ===");
    for (surface, a) in
        generate_noun_paradigm("Bauer", Gender::Masc, NounClass::WeakMasc, Some("Bauern"))
    {
        println!(
            "  {:>10}  {:?} {:?}",
            surface,
            a.features.case.unwrap(),
            a.features.number.unwrap()
        );
    }

    println!("\n=== Guessing for OOV \"Quitschung\" (made up -ung word) ===");
    for g in guess_noun("Quitschung") {
        println!(
            "  gender={:?} class={:?} confidence={:?}",
            g.gender, g.class, g.confidence
        );
    }

    println!("\n=== predict_dative_forms(\"Quitschung\") ===");
    for (form, conf) in predict_dative_forms("Quitschung") {
        println!("  {} (confidence: {:?})", form, conf);
    }

    println!("\n=== predict_dative_forms(\"Spezialist\") — weak masc suffix ===");
    for (form, conf) in predict_dative_forms("Spezialist") {
        println!("  {} (confidence: {:?})", form, conf);
    }

    println!(
        "\n=== predict_dative_forms(\"Quitsch\") — no suffix match, low confidence fallback ==="
    );
    for (form, conf) in predict_dative_forms("Quitsch") {
        println!("  {} (confidence: {:?})", form, conf);
    }

    println!("\n=== Honest failure: Bauer looks strong by surface but is weak in reality ===");
    for (form, conf) in predict_dative_forms("Bauer") {
        println!("  {} (confidence: {:?})", form, conf);
    }
    println!("  (the correct lexical answer is 'Bauern' — that requires FST lookup, not surface guessing)");
}
