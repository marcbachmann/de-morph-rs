//! German noun paradigm rules.
//!
//! Two entry points:
//! - [`generate_noun_paradigm`] — full paradigm from known
//!   `(lemma, gender, class, plural?)`. Deterministic.
//! - [`guess_noun`] — suffix-based class/gender hypotheses for an OOV
//!   lemma; [`predict_dative_forms`] composes guesses with paradigm
//!   generation to answer "what is the dative of this unknown word?".
//!
//! Return shape: `Vec<(String, Analysis)>` where the `String` is the
//! generated *surface* form and the `Analysis` carries the citation
//! `lemma` plus features and source tag. The two are kept separate
//! because, for example, "Tischen" is the surface form whose lemma is
//! "Tisch"; storing only one of them would lose information needed by
//! the FST build pipeline downstream.
//!
//! Notes on correctness:
//! - The German noun paradigm rules implemented here are facts of German
//!   grammar; they are not copyrightable. The maintainer learned them
//!   from standard general-knowledge sources; specific reference grammars
//!   (Duden, Helbig & Buscha) would carry the same rules but are not
//!   cited per-section because no copy was on hand when this file was
//!   written.
//! - The suffix heuristics for class/gender are widely-attested
//!   patterns ("-ung is feminine"). Confidence levels here are the
//!   maintainer's calibrated estimates, not formal frequencies measured
//!   against a corpus. A follow-up should measure these against the
//!   Wiktionary-derived lexicon and update the levels.
//!
//! Notes on performance:
//! - `generate_noun_paradigm` allocates exactly one `String` per output
//!   cell (8 typical), plus one `Analysis` heap allocation for the
//!   lemma. No regex, no parser; just suffix concatenation.
//! - `guess_noun` does one ASCII-lowercase allocation plus one
//!   `ends_with` per suffix rule. ~25 rules → sub-microsecond per call.

use crate::analysis::Analysis;
use crate::analysis::Case;
use crate::analysis::Features;
use crate::analysis::Gender;
use crate::analysis::Number;
use crate::analysis::UPOS;
use crate::analysis::Source;

/// Declension class for a German noun. Drives Genitiv-Singular formation
/// and (for `WeakMasc`) the dative/accusative-singular forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum NounClass {
    /// Strong declension: the default for most nouns.
    /// - Masc/Neut: Gen Sg adds -(e)s; Dat Sg = Nom Sg.
    /// - Fem: all singular cases are identical to the lemma.
    Strong = 0,
    /// Weak masculine ("n-stems"): every case except Nom Sg adds -(e)n.
    /// E.g. der Bauer → des/dem/den Bauern.
    WeakMasc = 1,
    /// Mixed declension: Nom Sg as lemma, Gen Sg as `-(e)ns`, all other
    /// non-Nom-Sg as `-(e)n`. Small closed class (Name, Glaube,
    /// Buchstabe, Friede, …).
    Mixed = 2,
    /// Singular only ("Singulariatantum"): die Milch, das Obst.
    SingulareTantum = 3,
    /// Plural only ("Pluraliatantum"): die Eltern, die Leute.
    PluraleTantum = 4,
}

/// Confidence ranking for a suffix-based class guess.
///
/// Numeric ordering: `High = 0` is best. Comparing with `<` therefore
/// gives "more confident than".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Confidence {
    High = 0,
    Medium = 1,
    Low = 2,
}

/// One hypothesis from the OOV guesser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NounGuess {
    pub gender: Gender,
    pub class: NounClass,
    pub confidence: Confidence,
}

/// One paradigm cell: surface form paired with its analysis.
///
/// `surface` is the inflected string (e.g. `"Tischen"`); the
/// `Analysis.lemma` carries the citation form (`"Tisch"`).
pub type ParadigmCell = (String, Analysis);

/// Generate the full noun paradigm.
///
/// `plural` is the Nom Pl form. If `None`, plural cases are omitted
/// unless the class itself implies one (WeakMasc derives its plural
/// from the oblique stem; PluraleTantum reuses the lemma).
///
/// All returned analyses are tagged [`Source::Generated`]; callers can
/// promote them to `Source::Lexicon` after attestation.
pub fn generate_noun_paradigm(
    lemma: &str,
    gender: Gender,
    class: NounClass,
    plural: Option<&str>,
) -> Vec<ParadigmCell> {
    let mut out = Vec::with_capacity(8);
    match class {
        NounClass::PluraleTantum => {
            let pl = plural.unwrap_or(lemma);
            push_plural_cases(&mut out, lemma, pl, gender);
        }
        NounClass::SingulareTantum => match gender {
            Gender::Fem => push_fem_singular(&mut out, lemma),
            Gender::Masc | Gender::Neut => push_strong_singular(&mut out, lemma, gender),
        },
        NounClass::WeakMasc => {
            push_weak_masc_singular(&mut out, lemma);
            let pl: String = plural
                .map(str::to_string)
                .unwrap_or_else(|| weak_masc_oblique(lemma));
            push_plural_cases(&mut out, lemma, &pl, gender);
        }
        NounClass::Mixed => {
            push_mixed_singular(&mut out, lemma, gender);
            if let Some(pl) = plural {
                push_plural_cases(&mut out, lemma, pl, gender);
            }
        }
        NounClass::Strong => match gender {
            Gender::Fem => {
                push_fem_singular(&mut out, lemma);
                if let Some(pl) = plural {
                    push_plural_cases(&mut out, lemma, pl, gender);
                }
            }
            Gender::Masc | Gender::Neut => {
                push_strong_singular(&mut out, lemma, gender);
                if let Some(pl) = plural {
                    push_plural_cases(&mut out, lemma, pl, gender);
                }
            }
        },
    }
    out
}

/// Suffix-based class/gender hypotheses for a lemma whose declension is
/// otherwise unknown.
///
/// Results are deduplicated by `(gender, class)` and sorted by
/// confidence (high first); on confidence tie, longer suffix matches
/// outrank shorter ones (i.e. more specific rules win). A fallback
/// `Strong Masc Low` hypothesis is appended if no rule matched.
pub fn guess_noun(lemma: &str) -> Vec<NounGuess> {
    let lower = lemma.to_lowercase();
    let mut matches: Vec<(usize, NounGuess)> = SUFFIX_RULES
        .iter()
        .filter(|r| lower.ends_with(r.suffix))
        .map(|r| {
            (
                r.suffix.len(),
                NounGuess {
                    gender: r.gender,
                    class: r.class,
                    confidence: r.confidence,
                },
            )
        })
        .collect();

    // Confidence ascending (High=0 first), then suffix length descending.
    matches.sort_by(|(la, a), (lb, b)| a.confidence.cmp(&b.confidence).then_with(|| lb.cmp(la)));

    let mut out: Vec<NounGuess> = Vec::with_capacity(matches.len() + 1);
    for (_, g) in matches {
        if !out
            .iter()
            .any(|h| h.gender == g.gender && h.class == g.class)
        {
            out.push(g);
        }
    }
    if out.is_empty() {
        out.push(NounGuess {
            gender: Gender::Masc,
            class: NounClass::Strong,
            confidence: Confidence::Low,
        });
    }
    out
}

/// Predict the dative forms (singular and plural) of an unknown noun.
///
/// Calls [`guess_noun`] to get a ranked list of class/gender hypotheses,
/// generates each hypothesis's paradigm, and extracts the Dat-Sg and
/// Dat-Pl cells. Returns `(surface, confidence)` pairs ordered by
/// confidence (best first), with duplicates collapsed to the
/// highest-confidence instance.
pub fn predict_dative_forms(lemma: &str) -> Vec<(String, Confidence)> {
    let mut by_form: Vec<(String, Confidence)> = Vec::new();
    for guess in guess_noun(lemma) {
        let plural = default_plural_guess(lemma, guess.gender, guess.class);
        let cells = generate_noun_paradigm(lemma, guess.gender, guess.class, plural.as_deref());
        for (surface, analysis) in cells {
            if analysis.features.case == Some(Case::Dat) {
                upsert_form(&mut by_form, &surface, guess.confidence);
            }
        }
    }
    by_form.sort_by(|a, b| a.1.cmp(&b.1));
    by_form
}

/// Dative-plural form from a nominative-plural form.
///
/// If the nom-pl already ends in -n or -s, no change. Otherwise append n.
/// Examples:
///   Tische  → Tischen        (regular)
///   Frauen  → Frauen         (already -en)
///   Bücher  → Büchern        (umlaut + -n)
///   Autos   → Autos          (loanword -s, no -n)
#[inline]
pub fn dative_plural(plural: &str) -> String {
    if plural.ends_with('n') || plural.ends_with('s') {
        plural.to_string()
    } else {
        format!("{plural}n")
    }
}

// =========================================================================
// Internals
// =========================================================================

#[inline]
fn push(out: &mut Vec<ParadigmCell>, surface: &str, lemma: &str, features: Features) {
    let analysis = Analysis::with_source(lemma, UPOS::NOUN, features, Source::Generated);
    out.push((surface.to_string(), analysis));
}

/// Strong-declension singular for masculine/neuter.
fn push_strong_singular(out: &mut Vec<ParadigmCell>, lemma: &str, gender: Gender) {
    push(
        out,
        lemma,
        lemma,
        Features::noun_form(gender, Number::Sg, Case::Nom),
    );
    for gen_form in genitive_singular_forms(lemma) {
        push(
            out,
            &gen_form,
            lemma,
            Features::noun_form(gender, Number::Sg, Case::Gen),
        );
    }
    push(
        out,
        lemma,
        lemma,
        Features::noun_form(gender, Number::Sg, Case::Dat),
    );
    push(
        out,
        lemma,
        lemma,
        Features::noun_form(gender, Number::Sg, Case::Acc),
    );
}

/// Feminine singular: all four cases collapse to the lemma.
fn push_fem_singular(out: &mut Vec<ParadigmCell>, lemma: &str) {
    for case in [Case::Nom, Case::Gen, Case::Dat, Case::Acc] {
        push(
            out,
            lemma,
            lemma,
            Features::noun_form(Gender::Fem, Number::Sg, case),
        );
    }
}

/// Weak-masculine singular: Nom Sg = lemma; Gen/Dat/Acc Sg add -(e)n.
fn push_weak_masc_singular(out: &mut Vec<ParadigmCell>, lemma: &str) {
    let oblique = weak_masc_oblique(lemma);
    push(
        out,
        lemma,
        lemma,
        Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
    );
    for case in [Case::Gen, Case::Dat, Case::Acc] {
        push(
            out,
            &oblique,
            lemma,
            Features::noun_form(Gender::Masc, Number::Sg, case),
        );
    }
}

/// Mixed-declension singular: Nom = lemma; Gen = +(e)ns; Dat/Acc = +(e)n.
fn push_mixed_singular(out: &mut Vec<ParadigmCell>, lemma: &str, gender: Gender) {
    let stem_n = weak_masc_oblique(lemma);
    let stem_ns = format!("{stem_n}s");
    push(
        out,
        lemma,
        lemma,
        Features::noun_form(gender, Number::Sg, Case::Nom),
    );
    push(
        out,
        &stem_ns,
        lemma,
        Features::noun_form(gender, Number::Sg, Case::Gen),
    );
    for case in [Case::Dat, Case::Acc] {
        push(
            out,
            &stem_n,
            lemma,
            Features::noun_form(gender, Number::Sg, case),
        );
    }
}

/// Plural cases. Dat Pl appends -n if the plural form doesn't already
/// end in -n or -s. This is one of the most reliable rules in German
/// morphology and is invariant across declension classes.
fn push_plural_cases(out: &mut Vec<ParadigmCell>, lemma: &str, plural: &str, gender: Gender) {
    let dat_pl = dative_plural(plural);
    for case in [Case::Nom, Case::Gen, Case::Acc] {
        push(
            out,
            plural,
            lemma,
            Features::noun_form(gender, Number::Pl, case),
        );
    }
    push(
        out,
        &dat_pl,
        lemma,
        Features::noun_form(gender, Number::Pl, Case::Dat),
    );
}

/// Genitive-singular forms for masculine/neuter strong nouns.
///
/// Returns one or two candidates. The rule is roughly:
///   - ends in s/ß/x/z, in -tz, or in -sch  → only `-es` (pronounceability)
///   - ends in unstressed -e                → only `-s`  (no double-e)
///   - otherwise                            → both `-(e)s` and `-s`
fn genitive_singular_forms(lemma: &str) -> Vec<String> {
    let l = lemma;
    let needs_es = l.ends_with('s')
        || l.ends_with('ß')
        || l.ends_with('x')
        || l.ends_with('z')
        || l.ends_with("tz")
        || l.ends_with("sch");
    if needs_es {
        return vec![format!("{l}es")];
    }
    if l.ends_with('e') {
        return vec![format!("{l}s")];
    }
    vec![format!("{l}es"), format!("{l}s")]
}

/// The oblique stem for a weak masculine (Gen/Dat/Acc Sg, and all plural
/// cases).
///
/// Rule:
///   - lemma ends in `-e`  → +n  (Junge → Jungen, Kollege → Kollegen)
///   - lemma ends in `-er` → +n  (Bauer → Bauern, Bayer → Bayern, Nachbar → Nachbarn)
///   - otherwise           → +en (Student → Studenten, Mensch → Menschen)
///
/// The `-er` clause covers the small native set of weak masculines
/// ending in unstressed -er. Foreign-derived weak masculines
/// (-ant/-ent/-ist/-oge/-aut) don't end in -e or -er, so they correctly
/// fall through to `+en`.
fn weak_masc_oblique(lemma: &str) -> String {
    if lemma.ends_with('e') || lemma.ends_with("er") {
        format!("{lemma}n")
    } else {
        format!("{lemma}en")
    }
}

/// Heuristic default plural for an OOV noun, given guessed gender/class.
///
/// Best-effort only. The German plural system is genuinely unpredictable
/// (Buch → Bücher, Auto → Autos, Tisch → Tische — no surface signal
/// reliably distinguishes them), so callers needing better should look
/// up the lemma in the lexicon and use the attested plural.
pub fn default_plural_guess(lemma: &str, gender: Gender, class: NounClass) -> Option<String> {
    match class {
        NounClass::SingulareTantum => None,
        NounClass::PluraleTantum => Some(lemma.to_string()),
        NounClass::WeakMasc => Some(weak_masc_oblique(lemma)),
        NounClass::Mixed => Some(weak_masc_oblique(lemma)),
        NounClass::Strong => Some(match gender {
            Gender::Fem => {
                // Feminine default: -(e)n (Frauen, Blumen).
                if lemma.ends_with('e') {
                    format!("{lemma}n")
                } else {
                    format!("{lemma}en")
                }
            }
            Gender::Masc => format!("{lemma}e"),
            Gender::Neut => format!("{lemma}e"),
        }),
    }
}

struct SuffixRule {
    suffix: &'static str,
    gender: Gender,
    class: NounClass,
    confidence: Confidence,
}

/// Suffix-based gender/class rules for German nouns.
///
/// Background: widely-attested patterns of German derivational
/// morphology. Any modern reference grammar enumerates them (e.g.
/// Duden Grammatik); no single section is cited because the maintainer
/// did not consult a specific reference while assembling this table.
const SUFFIX_RULES: &[SuffixRule] = &[
    // High-confidence feminine suffixes (essentially diagnostic).
    SuffixRule { suffix: "ung",    gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::High },
    SuffixRule { suffix: "heit",   gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::High },
    SuffixRule { suffix: "keit",   gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::High },
    SuffixRule { suffix: "schaft", gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::High },
    SuffixRule { suffix: "tät",    gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::High },
    SuffixRule { suffix: "ion",    gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::High },
    // Medium-confidence feminine.
    SuffixRule { suffix: "anz",    gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::Medium },
    SuffixRule { suffix: "enz",    gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::Medium },
    SuffixRule { suffix: "ie",     gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::Medium },
    SuffixRule { suffix: "ik",     gender: Gender::Fem,  class: NounClass::Strong,   confidence: Confidence::Medium },
    // Diminutives — almost always neuter.
    SuffixRule { suffix: "chen",   gender: Gender::Neut, class: NounClass::Strong,   confidence: Confidence::High },
    SuffixRule { suffix: "lein",   gender: Gender::Neut, class: NounClass::Strong,   confidence: Confidence::High },
    // Medium-confidence neuter.
    SuffixRule { suffix: "nis",    gender: Gender::Neut, class: NounClass::Strong,   confidence: Confidence::Medium },
    SuffixRule { suffix: "tum",    gender: Gender::Neut, class: NounClass::Strong,   confidence: Confidence::Medium },
    // Weak masculines (n-stems).
    SuffixRule { suffix: "ant",    gender: Gender::Masc, class: NounClass::WeakMasc, confidence: Confidence::High },
    SuffixRule { suffix: "ent",    gender: Gender::Masc, class: NounClass::WeakMasc, confidence: Confidence::High },
    SuffixRule { suffix: "ist",    gender: Gender::Masc, class: NounClass::WeakMasc, confidence: Confidence::High },
    SuffixRule { suffix: "oge",    gender: Gender::Masc, class: NounClass::WeakMasc, confidence: Confidence::High },
    // Strong masculines.
    SuffixRule { suffix: "ling",   gender: Gender::Masc, class: NounClass::Strong,   confidence: Confidence::High },
    SuffixRule { suffix: "or",     gender: Gender::Masc, class: NounClass::Strong,   confidence: Confidence::Medium },
];

/// Insert or merge a (form, confidence) entry, preserving the best
/// confidence for any given surface form.
#[inline]
fn upsert_form(by_form: &mut Vec<(String, Confidence)>, form: &str, conf: Confidence) {
    if let Some(slot) = by_form.iter_mut().find(|(s, _)| s == form) {
        if conf < slot.1 {
            slot.1 = conf;
        }
    } else {
        by_form.push((form.to_string(), conf));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surfaces<'a>(cells: &'a [ParadigmCell], case: Case, number: Number) -> Vec<&'a str> {
        cells
            .iter()
            .filter(|(_, a)| a.features.case == Some(case) && a.features.number == Some(number))
            .map(|(s, _)| s.as_str())
            .collect()
    }

    #[test]
    fn tisch_strong_masc() {
        let p = generate_noun_paradigm("Tisch", Gender::Masc, NounClass::Strong, Some("Tische"));
        assert_eq!(surfaces(&p, Case::Nom, Number::Sg), vec!["Tisch"]);
        assert_eq!(surfaces(&p, Case::Dat, Number::Sg), vec!["Tisch"]);
        assert_eq!(surfaces(&p, Case::Acc, Number::Sg), vec!["Tisch"]);
        assert_eq!(surfaces(&p, Case::Gen, Number::Sg), vec!["Tisches"]);
        assert_eq!(surfaces(&p, Case::Nom, Number::Pl), vec!["Tische"]);
        assert_eq!(surfaces(&p, Case::Gen, Number::Pl), vec!["Tische"]);
        assert_eq!(surfaces(&p, Case::Acc, Number::Pl), vec!["Tische"]);
        assert_eq!(surfaces(&p, Case::Dat, Number::Pl), vec!["Tischen"]);
        // Source tag and lemma carried through.
        assert!(p.iter().all(|(_, a)| a.source == Source::Generated));
        assert!(p.iter().all(|(_, a)| a.pos == UPOS::NOUN));
        assert!(p.iter().all(|(_, a)| a.lemma == "Tisch"));
        assert!(
            p.iter()
                .all(|(_, a)| a.features.gender == Some(Gender::Masc))
        );
    }

    #[test]
    fn hund_strong_masc_gen_sg_both_variants() {
        let p = generate_noun_paradigm("Hund", Gender::Masc, NounClass::Strong, Some("Hunde"));
        let gen_sg = surfaces(&p, Case::Gen, Number::Sg);
        assert!(gen_sg.contains(&"Hundes"));
        assert!(gen_sg.contains(&"Hunds"));
    }

    #[test]
    fn frau_strong_fem_all_singular_collapses() {
        let p = generate_noun_paradigm("Frau", Gender::Fem, NounClass::Strong, Some("Frauen"));
        for case in [Case::Nom, Case::Gen, Case::Dat, Case::Acc] {
            assert_eq!(surfaces(&p, case, Number::Sg), vec!["Frau"], "{case:?}");
        }
        // Frauen ends in -n, so Dat Pl = Nom Pl.
        assert_eq!(surfaces(&p, Case::Dat, Number::Pl), vec!["Frauen"]);
    }

    #[test]
    fn buch_strong_neut_umlaut_plural() {
        let p = generate_noun_paradigm("Buch", Gender::Neut, NounClass::Strong, Some("Bücher"));
        assert_eq!(surfaces(&p, Case::Dat, Number::Pl), vec!["Büchern"]);
        let gen_sg = surfaces(&p, Case::Gen, Number::Sg);
        assert!(gen_sg.contains(&"Buches"));
        assert!(gen_sg.contains(&"Buchs"));
    }

    #[test]
    fn bauer_weak_masc() {
        let p = generate_noun_paradigm("Bauer", Gender::Masc, NounClass::WeakMasc, Some("Bauern"));
        assert_eq!(surfaces(&p, Case::Nom, Number::Sg), vec!["Bauer"]);
        assert_eq!(surfaces(&p, Case::Gen, Number::Sg), vec!["Bauern"]);
        assert_eq!(surfaces(&p, Case::Dat, Number::Sg), vec!["Bauern"]);
        assert_eq!(surfaces(&p, Case::Acc, Number::Sg), vec!["Bauern"]);
        assert_eq!(surfaces(&p, Case::Nom, Number::Pl), vec!["Bauern"]);
        assert_eq!(surfaces(&p, Case::Dat, Number::Pl), vec!["Bauern"]);
    }

    #[test]
    fn junge_weak_masc_with_e_ending() {
        // Lemma ends in -e → oblique stem is +n, not +en.
        let p = generate_noun_paradigm("Junge", Gender::Masc, NounClass::WeakMasc, None);
        assert_eq!(surfaces(&p, Case::Dat, Number::Sg), vec!["Jungen"]);
        assert_eq!(surfaces(&p, Case::Acc, Number::Sg), vec!["Jungen"]);
    }

    #[test]
    fn eltern_pluraliatantum() {
        let p = generate_noun_paradigm("Eltern", Gender::Masc, NounClass::PluraleTantum, None);
        assert!(p.iter().all(|(_, a)| a.features.number == Some(Number::Pl)));
        for case in [Case::Nom, Case::Gen, Case::Dat, Case::Acc] {
            assert_eq!(surfaces(&p, case, Number::Pl), vec!["Eltern"]);
        }
    }

    #[test]
    fn milch_singulariatantum() {
        let p = generate_noun_paradigm("Milch", Gender::Fem, NounClass::SingulareTantum, None);
        assert!(p.iter().all(|(_, a)| a.features.number == Some(Number::Sg)));
        for case in [Case::Nom, Case::Gen, Case::Dat, Case::Acc] {
            assert_eq!(surfaces(&p, case, Number::Sg), vec!["Milch"]);
        }
    }

    #[test]
    fn dative_plural_rule_table() {
        // The single most important one-line rule in German morphology.
        assert_eq!(dative_plural("Tische"), "Tischen");
        assert_eq!(dative_plural("Frauen"), "Frauen");
        assert_eq!(dative_plural("Bücher"), "Büchern");
        assert_eq!(dative_plural("Autos"), "Autos");
        assert_eq!(dative_plural("Männer"), "Männern");
        assert_eq!(dative_plural("Kinder"), "Kindern");
    }

    #[test]
    fn guess_noun_zeitung_feminine() {
        let g = guess_noun("Zeitung");
        assert!(!g.is_empty());
        assert_eq!(g[0].gender, Gender::Fem);
        assert_eq!(g[0].class, NounClass::Strong);
        assert_eq!(g[0].confidence, Confidence::High);
    }

    #[test]
    fn guess_noun_maedchen_neuter() {
        let g = guess_noun("Mädchen");
        assert_eq!(g[0].gender, Gender::Neut);
        assert_eq!(g[0].confidence, Confidence::High);
    }

    #[test]
    fn guess_noun_tourist_weak_masc() {
        let g = guess_noun("Tourist");
        assert_eq!(g[0].gender, Gender::Masc);
        assert_eq!(g[0].class, NounClass::WeakMasc);
        assert_eq!(g[0].confidence, Confidence::High);
    }

    #[test]
    fn guess_noun_falls_back_to_strong_masc_low() {
        let g = guess_noun("Xyzzy");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].confidence, Confidence::Low);
    }

    #[test]
    fn predict_dative_returns_some_form_for_unknown_word() {
        // The user's question: an unknown word should still yield a
        // dative candidate. Correctness is not asserted for the made-up
        // input — only that the system answers rather than going silent.
        let forms = predict_dative_forms("Quitsch");
        assert!(!forms.is_empty(), "no dative candidates for 'Quitsch'");
    }

    #[test]
    fn predict_dative_for_known_suffix() {
        // For "-ung" lemmas the guesser commits to feminine strong;
        // Dat Sg = lemma, Dat Pl appends -en.
        let forms = predict_dative_forms("Fassung");
        let surfaces: Vec<&str> = forms.iter().map(|(s, _)| s.as_str()).collect();
        assert!(surfaces.contains(&"Fassung"), "missing Dat Sg in {forms:?}");
        assert!(
            surfaces.contains(&"Fassungen"),
            "missing Dat Pl in {forms:?}"
        );
        assert!(forms.iter().any(|(_, c)| *c == Confidence::High));
    }

    #[test]
    fn predict_dative_tagged_as_generated() {
        // The dative cells emitted via the paradigm generator should
        // carry Source::Generated downstream (verified separately via
        // generate_noun_paradigm — the predict_* helper drops the
        // analysis and only returns surfaces).
        let p = generate_noun_paradigm("Fassung", Gender::Fem, NounClass::Strong, Some("Fassungen"));
        let dat_sg = p
            .iter()
            .find(|(_, a)| a.features.case == Some(Case::Dat) && a.features.number == Some(Number::Sg))
            .unwrap();
        assert_eq!(dat_sg.1.source, Source::Generated);
    }
}
