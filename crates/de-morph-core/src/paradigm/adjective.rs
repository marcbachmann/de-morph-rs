//! German adjective paradigm rules.
//!
//! Takes the three attested forms from Wiktionary (Positiv, Komparativ,
//! Superlativ) and produces the full inflected paradigm:
//!
//! - **Predicative / uninflected**: the bare form for each degree (e.g.
//!   "groß", "größer", "am größten"). Tagged `Source::Attested`.
//! - **Attributive**: full case × number × gender × declension matrix
//!   for each degree. Tagged `Source::Inflected`.
//!
//! For an adjective with all three degrees the paradigm contains
//! roughly 3 predicative + 72 × 3 = 219 attributive cells; that's a
//! lot per adjective but the cells are short suffixes off a small
//! number of stems, and the FST minimisation collapses the shared
//! prefixes aggressively.
//!
//! Out of scope:
//! - Adjectives where the comparative or superlative is suppletive
//!   (`gut/besser/best`, `viel/mehr/meist`) — the stems are stored
//!   verbatim from Wiktionary, so suppletives that have a Komparativ
//!   field at all still work as predicative + attributive cell sets.
//! - Adjectives that lose -e in inflection because the stem already
//!   ends in unstressed -e (`leise` → `leiser`, attributive `leise`,
//!   `leisen`, …). The current implementation appends endings even
//!   when the stem already ends in -e, which yields "leisee" for
//!   strong Fem Sg Nom. Documenting as a known limitation; a small
//!   schwa-deletion rule is the planned fix.
//!
//! References (verified):
//! - Template documentation:
//!   <https://de.wiktionary.org/wiki/Vorlage:Deutsch_Adjektiv_%C3%9Cbersicht>
//! - Adjective declension tables: widely-attested standard German
//!   morphology; the maintainer did not consult a specific reference
//!   grammar while writing this file.

use crate::analysis::{Analysis, Case, Declension, Degree, Features, Gender, Number, Source, UPOS};

/// The Wiktionary-attested forms for one adjective.
#[derive(Debug, Clone, Default)]
pub struct AdjectiveAttested<'a> {
    /// Page title and lemma (the Positiv field).
    pub lemma: &'a str,
    /// The bare comparative form, already with `-er` suffix
    /// (e.g. "größer"). `None` for adjectives without comparison.
    pub komparativ: Option<&'a str>,
    /// The bare superlative form, already with `-en` suffix
    /// (e.g. "größten"), as Wiktionary stores it. The "am" prefix
    /// is not included.
    pub superlativ: Option<&'a str>,
}

/// One paradigm cell.
pub type AdjectiveCell = (String, Analysis);

/// Generate the full adjective paradigm.
pub fn generate_adjective_paradigm(inputs: &AdjectiveAttested) -> Vec<AdjectiveCell> {
    let mut out = Vec::with_capacity(220);
    let lemma = inputs.lemma;

    // Predicative / uninflected forms.
    push_predicative(&mut out, lemma, lemma, Degree::Pos, Source::Attested);
    if let Some(c) = inputs.komparativ {
        push_predicative(&mut out, c, lemma, Degree::Cmp, Source::Attested);
    }
    if let Some(s) = inputs.superlativ {
        push_predicative(&mut out, s, lemma, Degree::Sup, Source::Attested);
    }

    // Attributive forms — apply 72 endings to each degree's resolved
    // inflection stem. The positive stem's -el/-er elision is decided
    // with the Komparativ as evidence (see positive_attributive_stem);
    // the comparative/superlative stems come straight from attested
    // forms, so only final-e schwa-deletion applies to them.
    let pos_stem = positive_attributive_stem(lemma, inputs.komparativ);
    apply_all_endings(&mut out, &pos_stem, lemma, Degree::Pos);
    if let Some(c) = inputs.komparativ {
        apply_all_endings(&mut out, schwa_delete(c), lemma, Degree::Cmp);
    }
    if let Some(s) = inputs.superlativ {
        let stem = superlative_stem(s);
        apply_all_endings(&mut out, schwa_delete(&stem), lemma, Degree::Sup);
    }

    out
}

/// Bare/predicative form: degree set, all inflection-feature slots
/// empty. The lemma carried on the analysis is always the Positiv.
fn push_predicative(
    out: &mut Vec<AdjectiveCell>,
    surface: &str,
    lemma: &str,
    degree: Degree,
    source: Source,
) {
    let analysis = Analysis::with_source(
        lemma,
        UPOS::ADJ,
        Features {
            degree: Some(degree),
            ..Features::empty()
        },
        source,
    );
    out.push((surface.to_string(), analysis));
}

/// For one degree's stem, emit all 72 attributive cells (3 declensions
/// × 4 cases × 2 numbers × 3 genders).
///
/// `stem` is the already-resolved inflection stem for this degree — the
/// caller has applied schwa-deletion / -el/-er elision; this function
/// just concatenates endings.
fn apply_all_endings(out: &mut Vec<AdjectiveCell>, stem: &str, lemma: &str, degree: Degree) {
    use Case::*;
    use Declension::*;
    use Gender::*;
    use Number::*;

    let declensions = [Strong, Weak, Mixed];
    let cases = [Nom, Gen, Dat, Acc];
    let numbers = [Sg, Pl];
    let genders = [Masc, Fem, Neut];

    for &declension in &declensions {
        for &case in &cases {
            for &number in &numbers {
                for &gender in &genders {
                    let ending = adjective_ending(declension, case, number, gender);
                    let surface = format!("{stem}{ending}");
                    let analysis = Analysis::with_source(
                        lemma,
                        UPOS::ADJ,
                        Features {
                            degree: Some(degree),
                            declension: Some(declension),
                            case: Some(case),
                            number: Some(number),
                            gender: Some(gender),
                            ..Features::empty()
                        },
                        Source::Inflected,
                    );
                    out.push((surface, analysis));
                }
            }
        }
    }
}

/// Resolve the positive-degree attributive stem, deciding `-el` /
/// vowel+`-er` elision with the Komparativ as evidence.
///
/// [`elide_unstressed_medial_e`] flags stems whose *shape* could elide
/// (`dunkel`, `teuer`, `fidel`). Whether the medial `-e-` actually drops
/// depends on stress, which spelling doesn't reveal — but the attested
/// Komparativ does: `dunkel`→`dunkler` (elided) vs `fidel`→`fideler`
/// (kept). So when the Komparativ keeps the `-e-` (equals `{lemma}er`),
/// the stem is stressed and we do NOT elide; otherwise (elided,
/// umlauted, or no Komparativ at all) we trust the spelling heuristic
/// and elide.
///
/// Known residual: stressed `-el` adjectives with no Komparativ to
/// consult (`bumsfidel`, `kreuzfidel`) still elide wrongly. Rare enough
/// to leave for a curated exception list if it ever matters.
///
/// Stems with no elidable shape get plain final-`e` schwa-deletion
/// (`leise` → `leis`), applied to every degree by the callers.
fn positive_attributive_stem(lemma: &str, komparativ: Option<&str>) -> String {
    // "hoch" (and -hoch compounds: haushoch, turmhoch, …) inflect
    // attributively on the stem "hoh-": hohe / hoher / hohes. The bare
    // predicative keeps "hoch"; only the attributive forms drop the -c-.
    if let Some(prefix) = lemma.strip_suffix("hoch") {
        return format!("{prefix}hoh");
    }
    // Indeclinable -a color adjectives (rosa, lila) have no Standard
    // inflection; colloquially they take an -n- linker before the
    // ending: rosa → rosan- → rosane / rosaner / rosanes. Appending the
    // ending straight to the -a (rosaer) is not German. The bare
    // predicative "rosa" is emitted separately and stays uninflected.
    if lemma.ends_with('a') {
        return format!("{lemma}n");
    }
    if let Some(elided) = elide_unstressed_medial_e(lemma) {
        let komparativ_keeps_e = komparativ.is_some_and(|k| k == format!("{lemma}er"));
        if komparativ_keeps_e {
            return lemma.to_string();
        }
        return elided;
    }
    schwa_delete(lemma).to_string()
}

/// Drop the unstressed medial `-e-` of an `-el` / vowel+`-er` stem.
///
/// Returns `Some` only when the elision is reliably correct:
/// - `-el` preceded by a consonant other than `l` (`dunkel`→`dunkl`,
///   `edel`→`edl`, `nobel`→`nobl`). The `l` guard skips stressed
///   `parallel` (whose `-e-` stays), and double-`l` stems (`-ell`,
///   e.g. `aktuell`) never end in `-el` so they're excluded already.
/// - `-er` preceded by a vowel (`teuer`→`teur`, `sauer`→`saur`).
///   Consonant+`-er` (`bitter`, `finster`, `sauber`) keeps its `-e-`,
///   because `bittere` is the standard form there.
///
/// Returns `None` for every other stem (no elision).
fn elide_unstressed_medial_e(stem: &str) -> Option<String> {
    let chars: Vec<char> = stem.chars().collect();
    let n = chars.len();
    if n < 3 {
        return None;
    }
    // Final-stressed -el (the "fidel" family: fidel, bumsfidel,
    // kreuzfidel, quietschfidel) keeps its -e- (fidele, not fidle).
    // There's no general spelling signal for -el stress, but this is the
    // only common stressed-el class; any others that carry a Komparativ
    // are caught by the Komparativ check in positive_attributive_stem.
    if stem.ends_with("fidel") {
        return None;
    }
    let last = chars[n - 1];
    if chars[n - 2] != 'e' {
        return None;
    }
    let before = chars[n - 3];
    match last {
        'l' if !is_german_vowel(before) && before != 'l' => {}
        'r' if is_german_vowel(before) => {}
        _ => return None,
    }
    // Drop the `e` at position n-2; keep the final consonant.
    let mut out = String::with_capacity(stem.len());
    out.extend(&chars[..n - 2]);
    out.push(last);
    Some(out)
}

/// German vowels (including umlauts) for the schwa-elision rule.
fn is_german_vowel(c: char) -> bool {
    matches!(
        c,
        'a' | 'e'
            | 'i'
            | 'o'
            | 'u'
            | 'ä'
            | 'ö'
            | 'ü'
            | 'A'
            | 'E'
            | 'I'
            | 'O'
            | 'U'
            | 'Ä'
            | 'Ö'
            | 'Ü'
    )
}

/// Schwa-deletion: if the stem ends in a single unstressed `-e`, drop
/// it before appending an ending so we don't double the vowel.
///
/// Examples:
///   `"leise"` → `"leis"`  (then + `-e` → `"leise"`)
///   `"müde"`  → `"müd"`   (then + `-en` → `"müden"`)
///   `"böse"`  → `"bös"`   (then + `-er` → `"böser"`)
///   `"groß"`  → `"groß"`  (no change)
///
/// We leave stems ending in `-ee` (rare in adjectives; `Idee` is a
/// noun) untouched.
fn schwa_delete(stem: &str) -> &str {
    if stem.ends_with('e') && !stem.ends_with("ee") {
        &stem[..stem.len() - 1]
    } else {
        stem
    }
}

/// Strip the `-en` suffix from a Wiktionary Superlativ field to get
/// the bare superlative stem. If the field doesn't end in `-en`,
/// return it unchanged.
fn superlative_stem(superlativ: &str) -> String {
    superlativ
        .strip_suffix("en")
        .unwrap_or(superlativ)
        .to_string()
}

/// Lookup adjective ending for (declension, case, number, gender).
///
/// The big match below is the entire German adjective ending table.
/// Many cells collapse to the same string; explicit enumeration keeps
/// the source readable and matches reference grammars line-by-line.
fn adjective_ending(
    declension: Declension,
    case: Case,
    number: Number,
    gender: Gender,
) -> &'static str {
    use Case::*;
    use Declension::*;
    use Gender::*;
    use Number::*;

    match (declension, number, case, gender) {
        // -------------- STRONG -------------------------------------------
        (Strong, Sg, Nom, Masc) => "er",
        (Strong, Sg, Nom, Fem) => "e",
        (Strong, Sg, Nom, Neut) => "es",
        (Strong, Sg, Gen, Masc) => "en",
        (Strong, Sg, Gen, Fem) => "er",
        (Strong, Sg, Gen, Neut) => "en",
        (Strong, Sg, Dat, Masc) => "em",
        (Strong, Sg, Dat, Fem) => "er",
        (Strong, Sg, Dat, Neut) => "em",
        (Strong, Sg, Acc, Masc) => "en",
        (Strong, Sg, Acc, Fem) => "e",
        (Strong, Sg, Acc, Neut) => "es",
        (Strong, Pl, Nom, _) => "e",
        (Strong, Pl, Gen, _) => "er",
        (Strong, Pl, Dat, _) => "en",
        (Strong, Pl, Acc, _) => "e",
        // -------------- WEAK ---------------------------------------------
        (Weak, Sg, Nom, _) => "e",
        (Weak, Sg, Acc, Masc) => "en",
        (Weak, Sg, Acc, Fem) => "e",
        (Weak, Sg, Acc, Neut) => "e",
        (Weak, _, _, _) => "en",
        // -------------- MIXED --------------------------------------------
        // Mixed = Strong in Sg Nom/Acc Masc/Neut, weak everywhere else.
        // (The Fem singular Mixed has -e in Nom/Acc, matching both
        // strong and weak; we already covered the strong ones above.)
        (Mixed, Sg, Nom, Masc) => "er",
        (Mixed, Sg, Nom, Fem) => "e",
        (Mixed, Sg, Nom, Neut) => "es",
        (Mixed, Sg, Acc, Masc) => "en",
        (Mixed, Sg, Acc, Fem) => "e",
        (Mixed, Sg, Acc, Neut) => "es",
        (Mixed, _, _, _) => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gross() -> AdjectiveAttested<'static> {
        AdjectiveAttested {
            lemma: "groß",
            komparativ: Some("größer"),
            superlativ: Some("größten"),
        }
    }

    fn find(
        cells: &[AdjectiveCell],
        degree: Degree,
        declension: Option<Declension>,
        case: Option<Case>,
        number: Option<Number>,
        gender: Option<Gender>,
    ) -> Vec<&str> {
        cells
            .iter()
            .filter(|(_, a)| {
                a.features.degree == Some(degree)
                    && a.features.declension == declension
                    && a.features.case == case
                    && a.features.number == number
                    && a.features.gender == gender
            })
            .map(|(s, _)| s.as_str())
            .collect()
    }

    #[test]
    fn gross_full_paradigm_size() {
        // 3 predicative (UPOS, Cmp, Sup) + 3 × 72 attributive = 219 cells.
        let cells = generate_adjective_paradigm(&gross());
        assert_eq!(cells.len(), 219, "got {}", cells.len());
    }

    #[test]
    fn gross_predicative_forms() {
        let cells = generate_adjective_paradigm(&gross());
        let pos_pred = find(&cells, Degree::Pos, None, None, None, None);
        assert_eq!(pos_pred, vec!["groß"]);
        let cmp_pred = find(&cells, Degree::Cmp, None, None, None, None);
        assert_eq!(cmp_pred, vec!["größer"]);
        let sup_pred = find(&cells, Degree::Sup, None, None, None, None);
        assert_eq!(sup_pred, vec!["größten"]);
    }

    #[test]
    fn gross_strong_positive_endings() {
        let cells = generate_adjective_paradigm(&gross());
        let strong_pos = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(
            strong_pos(Case::Nom, Number::Sg, Gender::Masc),
            vec!["großer"]
        );
        assert_eq!(
            strong_pos(Case::Nom, Number::Sg, Gender::Fem),
            vec!["große"]
        );
        assert_eq!(
            strong_pos(Case::Nom, Number::Sg, Gender::Neut),
            vec!["großes"]
        );
        assert_eq!(
            strong_pos(Case::Dat, Number::Sg, Gender::Masc),
            vec!["großem"]
        );
        assert_eq!(
            strong_pos(Case::Gen, Number::Pl, Gender::Masc),
            vec!["großer"]
        );
        assert_eq!(
            strong_pos(Case::Dat, Number::Pl, Gender::Masc),
            vec!["großen"]
        );
    }

    #[test]
    fn gross_weak_positive_endings() {
        let cells = generate_adjective_paradigm(&gross());
        let weak_pos = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Weak),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(weak_pos(Case::Nom, Number::Sg, Gender::Masc), vec!["große"]);
        assert_eq!(weak_pos(Case::Nom, Number::Sg, Gender::Fem), vec!["große"]);
        assert_eq!(weak_pos(Case::Nom, Number::Sg, Gender::Neut), vec!["große"]);
        assert_eq!(
            weak_pos(Case::Acc, Number::Sg, Gender::Masc),
            vec!["großen"]
        );
        assert_eq!(weak_pos(Case::Acc, Number::Sg, Gender::Fem), vec!["große"]);
        assert_eq!(
            weak_pos(Case::Dat, Number::Pl, Gender::Masc),
            vec!["großen"]
        );
    }

    #[test]
    fn gross_mixed_matches_strong_in_nom_masc_neut() {
        let cells = generate_adjective_paradigm(&gross());
        let mixed = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Mixed),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        // Mixed Sg Nom Masc = Strong Sg Nom Masc = "großer".
        assert_eq!(mixed(Case::Nom, Number::Sg, Gender::Masc), vec!["großer"]);
        assert_eq!(mixed(Case::Nom, Number::Sg, Gender::Neut), vec!["großes"]);
        // Mixed Sg Gen = "en" (weak-like).
        assert_eq!(mixed(Case::Gen, Number::Sg, Gender::Masc), vec!["großen"]);
        // Mixed Pl = "en" (weak-like).
        assert_eq!(mixed(Case::Nom, Number::Pl, Gender::Masc), vec!["großen"]);
    }

    #[test]
    fn comparative_inflects_on_top_of_er() {
        let cells = generate_adjective_paradigm(&gross());
        let cmp_strong = |c, n, g| {
            find(
                &cells,
                Degree::Cmp,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        // "größer" + "er" Strong Sg Nom Masc.
        assert_eq!(
            cmp_strong(Case::Nom, Number::Sg, Gender::Masc),
            vec!["größerer"]
        );
        assert_eq!(
            cmp_strong(Case::Nom, Number::Sg, Gender::Fem),
            vec!["größere"]
        );
        assert_eq!(
            cmp_strong(Case::Dat, Number::Pl, Gender::Masc),
            vec!["größeren"]
        );
    }

    #[test]
    fn superlative_strips_en_then_inflects() {
        let cells = generate_adjective_paradigm(&gross());
        let sup_weak = |c, n, g| {
            find(
                &cells,
                Degree::Sup,
                Some(Declension::Weak),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        // "größten" - "en" = "größt"; + "e" Weak Sg Nom = "größte".
        assert_eq!(
            sup_weak(Case::Nom, Number::Sg, Gender::Masc),
            vec!["größte"]
        );
        assert_eq!(sup_weak(Case::Nom, Number::Sg, Gender::Fem), vec!["größte"]);
        assert_eq!(
            sup_weak(Case::Dat, Number::Pl, Gender::Masc),
            vec!["größten"]
        );
        // Strong Sg Nom Neut: "größtes".
        let sup_strong = |c, n, g| {
            find(
                &cells,
                Degree::Sup,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(
            sup_strong(Case::Nom, Number::Sg, Gender::Neut),
            vec!["größtes"]
        );
    }

    #[test]
    fn adjective_without_comparison_still_emits_positive() {
        let inputs = AdjectiveAttested {
            lemma: "tot",
            komparativ: None,
            superlativ: None,
        };
        let cells = generate_adjective_paradigm(&inputs);
        // 1 predicative + 72 attributive = 73 cells.
        assert_eq!(cells.len(), 73);
        assert!(cells
            .iter()
            .all(|(_, a)| a.features.degree == Some(Degree::Pos)));
    }

    #[test]
    fn predicative_carries_lexicon_source() {
        let cells = generate_adjective_paradigm(&gross());
        let pred = cells
            .iter()
            .find(|(s, _)| s == "groß")
            .expect("missing predicative groß");
        assert_eq!(pred.1.source, Source::Attested);
    }

    #[test]
    fn schwa_deletion_for_e_final_stem() {
        // "leise" — UPOS attributive should not double the -e.
        let inputs = AdjectiveAttested {
            lemma: "leise",
            komparativ: Some("leiser"),
            superlativ: Some("leisesten"),
        };
        let cells = generate_adjective_paradigm(&inputs);

        // Strong Sg Nom Fem: should be "leise", not "leisee".
        let strong_fem_nom = find(
            &cells,
            Degree::Pos,
            Some(Declension::Strong),
            Some(Case::Nom),
            Some(Number::Sg),
            Some(Gender::Fem),
        );
        assert_eq!(strong_fem_nom, vec!["leise"]);

        // Weak Sg Acc Masc: should be "leisen", not "leiseen".
        let weak_acc_masc = find(
            &cells,
            Degree::Pos,
            Some(Declension::Weak),
            Some(Case::Acc),
            Some(Number::Sg),
            Some(Gender::Masc),
        );
        assert_eq!(weak_acc_masc, vec!["leisen"]);

        // Strong Sg Nom Masc: "leiser" (the leise stem absorbs the -er ending).
        let strong_nom_masc = find(
            &cells,
            Degree::Pos,
            Some(Declension::Strong),
            Some(Case::Nom),
            Some(Number::Sg),
            Some(Gender::Masc),
        );
        assert_eq!(strong_nom_masc, vec!["leiser"]);
    }

    #[test]
    fn unstressed_el_stem_elides_in_positive_attributive() {
        // "dunkel" — German drops the unstressed medial -e- before any
        // inflectional ending: dunkle / dunkler / dunkles / dunklem /
        // dunklen. The bare predicative stays "dunkel". The comparative
        // ("dunkler", stored) and superlative ("dunkelst-") are NOT
        // touched by this rule.
        let inputs = AdjectiveAttested {
            lemma: "dunkel",
            komparativ: Some("dunkler"),
            superlativ: Some("dunkelsten"),
        };
        let cells = generate_adjective_paradigm(&inputs);
        let pos = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Fem), vec!["dunkle"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Masc), vec!["dunkler"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Neut), vec!["dunkles"]);
        assert_eq!(pos(Case::Dat, Number::Sg, Gender::Masc), vec!["dunklem"]);
        let weak_acc_masc = find(
            &cells,
            Degree::Pos,
            Some(Declension::Weak),
            Some(Case::Acc),
            Some(Number::Sg),
            Some(Gender::Masc),
        );
        assert_eq!(weak_acc_masc, vec!["dunklen"]);

        // The invalid un-elided forms must NOT appear anywhere.
        let surfaces: Vec<&str> = cells.iter().map(|(s, _)| s.as_str()).collect();
        for bad in ["dunkele", "dunkeler", "dunkelem", "dunkeles", "dunkelen"] {
            assert!(!surfaces.contains(&bad), "over-generated invalid {bad:?}");
        }

        // Predicative positive keeps the full stem.
        assert_eq!(
            find(&cells, Degree::Pos, None, None, None, None),
            vec!["dunkel"]
        );

        // Comparative must NOT be elided by the -er rule: "dunkler" + "e"
        // = "dunklere", not "dunkle".
        let cmp_fem_nom = find(
            &cells,
            Degree::Cmp,
            Some(Declension::Strong),
            Some(Case::Nom),
            Some(Number::Sg),
            Some(Gender::Fem),
        );
        assert_eq!(cmp_fem_nom, vec!["dunklere"]);

        // Superlative keeps its -e- ("dunkelste", not "dunklste").
        let sup_weak_masc_nom = find(
            &cells,
            Degree::Sup,
            Some(Declension::Weak),
            Some(Case::Nom),
            Some(Number::Sg),
            Some(Gender::Masc),
        );
        assert_eq!(sup_weak_masc_nom, vec!["dunkelste"]);
    }

    #[test]
    fn vowel_er_stem_elides_in_positive_attributive() {
        // "teuer" — the -e- after a vowel drops: teure / teurer / teures.
        let inputs = AdjectiveAttested {
            lemma: "teuer",
            komparativ: Some("teurer"),
            superlativ: Some("teuersten"),
        };
        let cells = generate_adjective_paradigm(&inputs);
        let pos = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Fem), vec!["teure"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Masc), vec!["teurer"]);
        assert_eq!(pos(Case::Dat, Number::Sg, Gender::Masc), vec!["teurem"]);
        let surfaces: Vec<&str> = cells.iter().map(|(s, _)| s.as_str()).collect();
        for bad in ["teuere", "teuerer", "teuerem"] {
            assert!(!surfaces.contains(&bad), "over-generated invalid {bad:?}");
        }
    }

    #[test]
    fn indeclinable_a_adjective_takes_n_linker() {
        // rosa / lila are uninflected in Standard German; colloquially
        // they inflect with an -n- linker (rosane / rosaner / rosanes),
        // NOT by appending the ending straight to the -a (the invalid
        // rosae / rosaer). The bare predicative stays "rosa".
        let inputs = AdjectiveAttested {
            lemma: "rosa",
            komparativ: None,
            superlativ: None,
        };
        let cells = generate_adjective_paradigm(&inputs);
        let pos = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Fem), vec!["rosane"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Masc), vec!["rosaner"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Neut), vec!["rosanes"]);
        let surfaces: Vec<&str> = cells.iter().map(|(s, _)| s.as_str()).collect();
        for bad in ["rosae", "rosaer", "rosaes", "rosaem", "rosaen"] {
            assert!(!surfaces.contains(&bad), "over-generated invalid {bad:?}");
        }
        assert_eq!(
            find(&cells, Degree::Pos, None, None, None, None),
            vec!["rosa"]
        );
    }

    #[test]
    fn hoch_uses_hoh_stem_in_attributive() {
        // "hoch" inflects attributively on the stem "hoh-": hohe / hoher
        // / hohes / hohem / hohen. The bare predicative stays "hoch"; the
        // comparative (höher) and superlative (höchst-) are unaffected.
        let inputs = AdjectiveAttested {
            lemma: "hoch",
            komparativ: Some("höher"),
            superlativ: Some("höchsten"),
        };
        let cells = generate_adjective_paradigm(&inputs);
        let pos = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Fem), vec!["hohe"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Masc), vec!["hoher"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Neut), vec!["hohes"]);
        assert_eq!(pos(Case::Dat, Number::Sg, Gender::Masc), vec!["hohem"]);
        let surfaces: Vec<&str> = cells.iter().map(|(s, _)| s.as_str()).collect();
        for bad in ["hoche", "hocher", "hoches", "hochem", "hochen"] {
            assert!(!surfaces.contains(&bad), "over-generated invalid {bad:?}");
        }
        assert_eq!(
            find(&cells, Degree::Pos, None, None, None, None),
            vec!["hoch"]
        );
    }

    #[test]
    fn hoch_compound_uses_hoh_stem() {
        // -hoch compounds inflect the same way: haushoch → haushohe.
        let inputs = AdjectiveAttested {
            lemma: "haushoch",
            komparativ: None,
            superlativ: None,
        };
        let cells = generate_adjective_paradigm(&inputs);
        let fem_nom = find(
            &cells,
            Degree::Pos,
            Some(Declension::Strong),
            Some(Case::Nom),
            Some(Number::Sg),
            Some(Gender::Fem),
        );
        assert_eq!(fem_nom, vec!["haushohe"]);
        assert!(!cells.iter().any(|(s, _)| s == "haushoche"));
    }

    #[test]
    fn stressed_el_with_unelided_komparativ_is_not_elided() {
        // "fidel" is stressed on the -el, so it does NOT elide: fidele /
        // fideler. Wiktionary's Komparativ ("fideler", not "fidler")
        // attests this, and we trust it over the spelling heuristic
        // (which, on shape alone, would wrongly elide to "fidle").
        let inputs = AdjectiveAttested {
            lemma: "fidel",
            komparativ: Some("fideler"),
            superlativ: Some("fidelsten"),
        };
        let cells = generate_adjective_paradigm(&inputs);
        let pos = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Fem), vec!["fidele"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Masc), vec!["fideler"]);
        let surfaces: Vec<&str> = cells.iter().map(|(s, _)| s.as_str()).collect();
        assert!(!surfaces.contains(&"fidle"), "wrongly elided fidel→fidle");
    }

    #[test]
    fn stressed_fidel_family_not_elided_without_komparativ() {
        // "bumsfidel"/"kreuzfidel"/"quietschfidel" are compounds of the
        // final-stressed "fidel"; they keep the -e- (bumsfidele), but
        // have no Komparativ in Wiktionary to attest it. The fidel-family
        // guard handles them on shape alone.
        for lemma in ["bumsfidel", "kreuzfidel", "quietschfidel"] {
            let inputs = AdjectiveAttested {
                lemma,
                komparativ: None,
                superlativ: None,
            };
            let cells = generate_adjective_paradigm(&inputs);
            let fem_nom = find(
                &cells,
                Degree::Pos,
                Some(Declension::Strong),
                Some(Case::Nom),
                Some(Number::Sg),
                Some(Gender::Fem),
            );
            assert_eq!(fem_nom, vec![format!("{lemma}e")], "{lemma}");
            assert!(
                !cells
                    .iter()
                    .any(|(s, _)| s == &lemma.replace("fidel", "fidl")),
                "wrongly elided {lemma}"
            );
        }
    }

    #[test]
    fn stressed_el_stem_is_not_elided() {
        // "parallel" is stressed on the final syllable; the -e- stays:
        // parallele / paralleler. (Eliding would yield garbage "parallle".)
        let inputs = AdjectiveAttested {
            lemma: "parallel",
            komparativ: None,
            superlativ: None,
        };
        let cells = generate_adjective_paradigm(&inputs);
        let pos = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Fem), vec!["parallele"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Masc), vec!["paralleler"]);
    }

    #[test]
    fn consonant_er_stem_is_not_elided() {
        // "bitter" — the -e- after a consonant stays: "bittere",
        // "bitterer" are the standard forms (eliding to "bittre" is at
        // best poetic and would drop the standard form).
        let inputs = AdjectiveAttested {
            lemma: "bitter",
            komparativ: None,
            superlativ: None,
        };
        let cells = generate_adjective_paradigm(&inputs);
        let pos = |c, n, g| {
            find(
                &cells,
                Degree::Pos,
                Some(Declension::Strong),
                Some(c),
                Some(n),
                Some(g),
            )
        };
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Fem), vec!["bittere"]);
        assert_eq!(pos(Case::Nom, Number::Sg, Gender::Masc), vec!["bitterer"]);
    }

    #[test]
    fn schwa_deletion_leaves_consonant_stems_alone() {
        // "groß" doesn't end in -e; no schwa-deletion.
        let cells = generate_adjective_paradigm(&gross());
        let strong_fem_nom = find(
            &cells,
            Degree::Pos,
            Some(Declension::Strong),
            Some(Case::Nom),
            Some(Number::Sg),
            Some(Gender::Fem),
        );
        assert_eq!(strong_fem_nom, vec!["große"]);
    }

    #[test]
    fn attributive_carries_generated_source() {
        let cells = generate_adjective_paradigm(&gross());
        let attr = cells
            .iter()
            .find(|(_, a)| {
                a.features.degree == Some(Degree::Pos)
                    && a.features.declension == Some(Declension::Strong)
                    && a.features.case == Some(Case::Nom)
                    && a.features.number == Some(Number::Sg)
                    && a.features.gender == Some(Gender::Masc)
            })
            .expect("missing strong Sg Nom Masc");
        assert_eq!(attr.0, "großer");
        assert_eq!(attr.1.source, Source::Inflected);
    }
}
