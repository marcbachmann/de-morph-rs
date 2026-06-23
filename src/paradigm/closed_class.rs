//! Hard-coded closed-class word entries: personal pronouns, definite
//! and indefinite articles, and the negation determiner `kein`.
//!
//! These items are not extracted from Wiktionary because:
//! 1. The set is small (~80 entries total) and stable over centuries;
//! 2. Wiktionary doesn't have a uniform "overview" template for these
//!    classes — they're documented as plain prose;
//! 3. Hand-curation guarantees correctness, including the tricky
//!    syncretisms ("sie" = 3Sg Fem Nom AND 3Pl Nom AND ...).
//!
//! All entries are tagged [`Source::Attested`] because they're directly
//! curated by the maintainer from the standard German grammar.
//!
//! References (background, no specific section consulted while writing):
//! - Personal pronoun paradigm and case syncretisms: standard German
//!   grammar; any reference grammar (Duden, Helbig & Buscha) has the
//!   same table.
//! - Article paradigms (der, ein, kein): same.

use crate::analysis::{Analysis, Case, Features, Gender, Number, Person, PronType, Source, UPOS};

/// One generated closed-class entry.
pub type ClosedClassEntry = (String, Analysis);

/// Generate all closed-class entries.
pub fn generate_closed_class_entries() -> Vec<ClosedClassEntry> {
    let mut out = Vec::with_capacity(800);
    add_personal_pronouns(&mut out);
    add_reflexive_pronouns(&mut out);
    add_definite_article(&mut out);
    add_relative_pronouns(&mut out);
    add_ein_pattern(&mut out, "ein", false, Some(PronType::Art), None, None);
    add_ein_pattern(&mut out, "kein", true, Some(PronType::Neg), None, None);
    // Possessive determiners. Possessor person/number tracked per
    // lemma — `mein` (1Sg), `dein` (2Sg), `sein` (3Sg), `unser` (1Pl),
    // `euer` (2Pl), `Ihr` (polite 2nd). `ihr` is ambiguous between
    // 3Sg-Fem and 3Pl; we emit it once with `poss_person = Some(P3)`
    // and the possessor number left `None`.
    for &(lemma, poss_person, poss_number) in &[
        ("mein", Person::P1, Some(Number::Sg)),
        ("dein", Person::P2, Some(Number::Sg)),
        ("sein", Person::P3, Some(Number::Sg)),
        ("ihr", Person::P3, None),
        ("unser", Person::P1, Some(Number::Pl)),
        ("euer", Person::P2, Some(Number::Pl)),
        ("Ihr", Person::P2, None),
    ] {
        add_ein_pattern(
            &mut out,
            lemma,
            true,
            Some(PronType::Prs),
            Some(poss_person),
            poss_number,
        );
    }
    // Vowel-reduced alternative forms for euer and unser: euer + e →
    // "eure" (alongside "euere"); unser + e → "unsere" (already the
    // standard) but also unsre/unsres/etc. See add_vowel_reduced_pattern.
    add_vowel_reduced_pattern(&mut out, "euer", "eur", Person::P2, Some(Number::Pl));
    add_vowel_reduced_pattern(&mut out, "unser", "unsr", Person::P1, Some(Number::Pl));
    // Demonstrative and quantifying determiners. Strong-adjective endings.
    for demonstrative in &["dieser", "jener", "jeder", "mancher", "welcher", "solcher"] {
        add_demonstrative_pattern(&mut out, demonstrative);
    }
    add_interrogative_pronouns(&mut out);
    add_indefinite_pronouns(&mut out);
    add_numerals(&mut out);
    add_ordinals(&mut out);
    add_compound_cardinals(&mut out);
    add_conjunctions(&mut out);
    add_prepositions(&mut out);
    add_punctuation(&mut out);
    out
}

// ---------------------------------------------------------------------------
// Personal pronouns
// ---------------------------------------------------------------------------

/// Each tuple: surface, lemma, person, number, optional gender, case.
const PERSONAL_PRONOUNS: &[(&str, &str, Person, Number, Option<Gender>, Case)] = &[
    // 1Sg (lemma: ich)
    ("ich", "ich", Person::P1, Number::Sg, None, Case::Nom),
    ("meiner", "ich", Person::P1, Number::Sg, None, Case::Gen),
    ("mir", "ich", Person::P1, Number::Sg, None, Case::Dat),
    ("mich", "ich", Person::P1, Number::Sg, None, Case::Acc),
    // 2Sg (lemma: du)
    ("du", "du", Person::P2, Number::Sg, None, Case::Nom),
    ("deiner", "du", Person::P2, Number::Sg, None, Case::Gen),
    ("dir", "du", Person::P2, Number::Sg, None, Case::Dat),
    ("dich", "du", Person::P2, Number::Sg, None, Case::Acc),
    // 3Sg Masc (lemma: er)
    (
        "er",
        "er",
        Person::P3,
        Number::Sg,
        Some(Gender::Masc),
        Case::Nom,
    ),
    (
        "seiner",
        "er",
        Person::P3,
        Number::Sg,
        Some(Gender::Masc),
        Case::Gen,
    ),
    (
        "ihm",
        "er",
        Person::P3,
        Number::Sg,
        Some(Gender::Masc),
        Case::Dat,
    ),
    (
        "ihn",
        "er",
        Person::P3,
        Number::Sg,
        Some(Gender::Masc),
        Case::Acc,
    ),
    // 3Sg Fem (lemma: sie)
    (
        "sie",
        "sie",
        Person::P3,
        Number::Sg,
        Some(Gender::Fem),
        Case::Nom,
    ),
    (
        "ihrer",
        "sie",
        Person::P3,
        Number::Sg,
        Some(Gender::Fem),
        Case::Gen,
    ),
    (
        "ihr",
        "sie",
        Person::P3,
        Number::Sg,
        Some(Gender::Fem),
        Case::Dat,
    ),
    (
        "sie",
        "sie",
        Person::P3,
        Number::Sg,
        Some(Gender::Fem),
        Case::Acc,
    ),
    // 3Sg Neut (lemma: es)
    (
        "es",
        "es",
        Person::P3,
        Number::Sg,
        Some(Gender::Neut),
        Case::Nom,
    ),
    (
        "seiner",
        "es",
        Person::P3,
        Number::Sg,
        Some(Gender::Neut),
        Case::Gen,
    ),
    (
        "ihm",
        "es",
        Person::P3,
        Number::Sg,
        Some(Gender::Neut),
        Case::Dat,
    ),
    (
        "es",
        "es",
        Person::P3,
        Number::Sg,
        Some(Gender::Neut),
        Case::Acc,
    ),
    // 1Pl (lemma: wir)
    ("wir", "wir", Person::P1, Number::Pl, None, Case::Nom),
    ("unser", "wir", Person::P1, Number::Pl, None, Case::Gen),
    ("uns", "wir", Person::P1, Number::Pl, None, Case::Dat),
    ("uns", "wir", Person::P1, Number::Pl, None, Case::Acc),
    // 2Pl (lemma: ihr)
    ("ihr", "ihr", Person::P2, Number::Pl, None, Case::Nom),
    ("euer", "ihr", Person::P2, Number::Pl, None, Case::Gen),
    ("euch", "ihr", Person::P2, Number::Pl, None, Case::Dat),
    ("euch", "ihr", Person::P2, Number::Pl, None, Case::Acc),
    // 3Pl (lemma: sie)
    ("sie", "sie", Person::P3, Number::Pl, None, Case::Nom),
    ("ihrer", "sie", Person::P3, Number::Pl, None, Case::Gen),
    ("ihnen", "sie", Person::P3, Number::Pl, None, Case::Dat),
    ("sie", "sie", Person::P3, Number::Pl, None, Case::Acc),
    // Polite 2nd person (lemma: Sie) — same forms as 3Pl but capitalised.
    ("Sie", "Sie", Person::P2, Number::Pl, None, Case::Nom),
    ("Ihrer", "Sie", Person::P2, Number::Pl, None, Case::Gen),
    ("Ihnen", "Sie", Person::P2, Number::Pl, None, Case::Dat),
    ("Sie", "Sie", Person::P2, Number::Pl, None, Case::Acc),
];

fn add_personal_pronouns(out: &mut Vec<ClosedClassEntry>) {
    for &(surface, lemma, person, number, gender, case) in PERSONAL_PRONOUNS {
        let features = Features {
            person: Some(person),
            number: Some(number),
            gender,
            case: Some(case),
            pron_type: Some(PronType::Prs),
            ..Features::empty()
        };
        out.push((
            surface.to_string(),
            Analysis::with_source(lemma, UPOS::PRON, features, Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// Definite article (der/die/das)
// ---------------------------------------------------------------------------

/// Tuple: (number, case, gender, surface). The definite-article paradigm
/// is irregular enough to warrant explicit enumeration.
const DER_PARADIGM: &[(Number, Case, Option<Gender>, &str)] = &[
    (Number::Sg, Case::Nom, Some(Gender::Masc), "der"),
    (Number::Sg, Case::Nom, Some(Gender::Fem), "die"),
    (Number::Sg, Case::Nom, Some(Gender::Neut), "das"),
    (Number::Sg, Case::Gen, Some(Gender::Masc), "des"),
    (Number::Sg, Case::Gen, Some(Gender::Fem), "der"),
    (Number::Sg, Case::Gen, Some(Gender::Neut), "des"),
    (Number::Sg, Case::Dat, Some(Gender::Masc), "dem"),
    (Number::Sg, Case::Dat, Some(Gender::Fem), "der"),
    (Number::Sg, Case::Dat, Some(Gender::Neut), "dem"),
    (Number::Sg, Case::Acc, Some(Gender::Masc), "den"),
    (Number::Sg, Case::Acc, Some(Gender::Fem), "die"),
    (Number::Sg, Case::Acc, Some(Gender::Neut), "das"),
    // Plural is gender-invariant.
    (Number::Pl, Case::Nom, None, "die"),
    (Number::Pl, Case::Gen, None, "der"),
    (Number::Pl, Case::Dat, None, "den"),
    (Number::Pl, Case::Acc, None, "die"),
];

fn add_definite_article(out: &mut Vec<ClosedClassEntry>) {
    for &(number, case, gender, surface) in DER_PARADIGM {
        let features = Features {
            number: Some(number),
            case: Some(case),
            gender,
            pron_type: Some(PronType::Art),
            ..Features::empty()
        };
        out.push((
            surface.to_string(),
            Analysis::with_source("der", UPOS::DET, features, Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// ein / kein pattern
// ---------------------------------------------------------------------------

/// The ein-pattern singular endings (gender × case suffix). `""` means the
/// bare stem; otherwise the stem + suffix.
const EIN_PATTERN_SG: &[(Case, Gender, &str)] = &[
    (Case::Nom, Gender::Masc, ""),
    (Case::Nom, Gender::Fem, "e"),
    (Case::Nom, Gender::Neut, ""),
    (Case::Gen, Gender::Masc, "es"),
    (Case::Gen, Gender::Fem, "er"),
    (Case::Gen, Gender::Neut, "es"),
    (Case::Dat, Gender::Masc, "em"),
    (Case::Dat, Gender::Fem, "er"),
    (Case::Dat, Gender::Neut, "em"),
    (Case::Acc, Gender::Masc, "en"),
    (Case::Acc, Gender::Fem, "e"),
    (Case::Acc, Gender::Neut, ""),
];

/// Plural endings for the ein-pattern (only used for `kein` and the
/// possessives — `ein` itself has no plural).
const EIN_PATTERN_PL: &[(Case, &str)] = &[
    (Case::Nom, "e"),
    (Case::Gen, "er"),
    (Case::Dat, "en"),
    (Case::Acc, "e"),
];

fn add_ein_pattern(
    out: &mut Vec<ClosedClassEntry>,
    lemma: &str,
    has_plural: bool,
    pron_type: Option<PronType>,
    poss_person: Option<Person>,
    poss_number: Option<Number>,
) {
    for &(case, gender, suffix) in EIN_PATTERN_SG {
        let surface = format!("{lemma}{suffix}");
        let features = Features {
            number: Some(Number::Sg),
            case: Some(case),
            gender: Some(gender),
            pron_type,
            poss_person,
            poss_number,
            ..Features::empty()
        };
        out.push((
            surface,
            Analysis::with_source(lemma, UPOS::DET, features, Source::Attested),
        ));
    }
    if has_plural {
        for &(case, suffix) in EIN_PATTERN_PL {
            let surface = format!("{lemma}{suffix}");
            let features = Features {
                number: Some(Number::Pl),
                case: Some(case),
                pron_type,
                poss_person,
                poss_number,
                ..Features::empty()
            };
            out.push((
                surface,
                Analysis::with_source(lemma, UPOS::DET, features, Source::Attested),
            ));
        }
    }
}

/// Emit vowel-reduced alternative forms for possessives like `euer` and
/// `unser`. Standard German allows both the full and reduced surfaces
/// (`eure` ≡ `euere`, `unsre` ≡ `unsere`); we emit the reduced shape as
/// an additional entry. The lemma stays the original full form.
fn add_vowel_reduced_pattern(
    out: &mut Vec<ClosedClassEntry>,
    lemma: &str,
    reduced_stem: &str,
    poss_person: Person,
    poss_number: Option<Number>,
) {
    for &(case, gender, suffix) in EIN_PATTERN_SG {
        if suffix.is_empty() {
            // Bare-stem cell coincides with the full lemma surface; no
            // separate reduced form exists.
            continue;
        }
        let surface = format!("{reduced_stem}{suffix}");
        let features = Features {
            number: Some(Number::Sg),
            case: Some(case),
            gender: Some(gender),
            pron_type: Some(PronType::Prs),
            poss_person: Some(poss_person),
            poss_number,
            ..Features::empty()
        };
        out.push((
            surface,
            Analysis::with_source(lemma, UPOS::DET, features, Source::Attested),
        ));
    }
    for &(case, suffix) in EIN_PATTERN_PL {
        if suffix.is_empty() {
            continue;
        }
        let surface = format!("{reduced_stem}{suffix}");
        let features = Features {
            number: Some(Number::Pl),
            case: Some(case),
            pron_type: Some(PronType::Prs),
            poss_person: Some(poss_person),
            poss_number,
            ..Features::empty()
        };
        out.push((
            surface,
            Analysis::with_source(lemma, UPOS::DET, features, Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// Reflexive pronouns
// ---------------------------------------------------------------------------

/// Reflexive paradigm. Surfaces overlap with the personal pronoun
/// paradigm for 1Sg/2Sg/1Pl/2Pl (mich/dich/uns/euch), but the lemma is
/// always "sich" (the citation form for the reflexive series in
/// standard reference grammars). The analyzer therefore returns BOTH
/// personal and reflexive readings for those shared surfaces, and the
/// caller picks by context.
const REFLEXIVE_PRONOUNS: &[(&str, Person, Number, Case)] = &[
    ("mich", Person::P1, Number::Sg, Case::Acc),
    ("mir", Person::P1, Number::Sg, Case::Dat),
    ("dich", Person::P2, Number::Sg, Case::Acc),
    ("dir", Person::P2, Number::Sg, Case::Dat),
    ("sich", Person::P3, Number::Sg, Case::Acc),
    ("sich", Person::P3, Number::Sg, Case::Dat),
    ("uns", Person::P1, Number::Pl, Case::Acc),
    ("uns", Person::P1, Number::Pl, Case::Dat),
    ("euch", Person::P2, Number::Pl, Case::Acc),
    ("euch", Person::P2, Number::Pl, Case::Dat),
    ("sich", Person::P3, Number::Pl, Case::Acc),
    ("sich", Person::P3, Number::Pl, Case::Dat),
];

fn add_reflexive_pronouns(out: &mut Vec<ClosedClassEntry>) {
    for &(surface, person, number, case) in REFLEXIVE_PRONOUNS {
        let features = Features {
            person: Some(person),
            number: Some(number),
            case: Some(case),
            pron_type: Some(PronType::Refl),
            ..Features::empty()
        };
        out.push((
            surface.to_string(),
            Analysis::with_source("sich", UPOS::PRON, features, Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// Relative pronouns (der / die / das used as relatives)
// ---------------------------------------------------------------------------

/// Relative pronouns share their surface forms with the definite
/// article (and `welcher` with the interrogative determiner). To make
/// the relative reading retrievable, we emit a parallel set of
/// entries with `UPOS::PRON` and lemma `"der"` (the canonical relative
/// citation form in German reference grammars). The Genitive forms
/// for relative use are *dessen* (Masc/Neut Sg) and *deren* (Fem Sg,
/// Pl), which differ from the article's `des`/`der` — those are
/// included here as the relative-specific surface forms.
fn add_relative_pronouns(out: &mut Vec<ClosedClassEntry>) {
    // Same forms as the definite-article paradigm for Nom/Dat/Acc...
    for &(number, case, gender, surface) in DER_PARADIGM {
        // ...but skip the Genitive cells; they're overridden below.
        if case == Case::Gen {
            continue;
        }
        let features = Features {
            number: Some(number),
            case: Some(case),
            gender,
            pron_type: Some(PronType::Rel),
            ..Features::empty()
        };
        out.push((
            surface.to_string(),
            Analysis::with_source("der", UPOS::PRON, features, Source::Attested),
        ));
    }
    // Relative-specific Genitive forms.
    let rel_gen: &[(Number, Option<Gender>, &str)] = &[
        (Number::Sg, Some(Gender::Masc), "dessen"),
        (Number::Sg, Some(Gender::Neut), "dessen"),
        (Number::Sg, Some(Gender::Fem), "deren"),
        (Number::Pl, None, "deren"),
    ];
    for &(number, gender, surface) in rel_gen {
        let features = Features {
            number: Some(number),
            case: Some(Case::Gen),
            gender,
            pron_type: Some(PronType::Rel),
            ..Features::empty()
        };
        out.push((
            surface.to_string(),
            Analysis::with_source("der", UPOS::PRON, features, Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// Interrogative pronouns (wer / was)
// ---------------------------------------------------------------------------

/// (surface, lemma, case) tuples for the interrogative paradigm.
///
/// `wer` declines for case only (no gender, no number — used to ask
/// about persons of any gender, singular or plural).
/// `was` is the thing-interrogative; standard German uses it for Nom
/// and Acc only, with "wessen" optionally serving the Gen slot. There
/// is no native Dat form for `was`; prepositional `wo-` compounds
/// ("womit", "wodurch", "worauf") fill that role at the syntactic
/// layer.
const INTERROGATIVE_PRONOUNS: &[(&str, &str, Case)] = &[
    // wer: who (persons)
    ("wer", "wer", Case::Nom),
    ("wessen", "wer", Case::Gen),
    ("wem", "wer", Case::Dat),
    ("wen", "wer", Case::Acc),
    // was: what (things)
    ("was", "was", Case::Nom),
    ("wessen", "was", Case::Gen),
    ("was", "was", Case::Acc),
];

fn add_interrogative_pronouns(out: &mut Vec<ClosedClassEntry>) {
    for &(surface, lemma, case) in INTERROGATIVE_PRONOUNS {
        let features = Features {
            case: Some(case),
            pron_type: Some(PronType::Int),
            ..Features::empty()
        };
        out.push((
            surface.to_string(),
            Analysis::with_source(lemma, UPOS::PRON, features, Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// Indefinite pronouns (jemand / niemand / etwas / nichts / man / alle / ...)
// ---------------------------------------------------------------------------

fn add_indefinite_pronouns(out: &mut Vec<ClosedClassEntry>) {
    // jemand / niemand — singular paradigm. The -en and -em endings
    // are increasingly often dropped in colloquial German; we emit both
    // the formal and the bare-stem Acc form.
    for &lemma in &["jemand", "niemand"] {
        for &(case, suffix) in &[
            (Case::Nom, ""),
            (Case::Gen, "es"),
            (Case::Dat, "em"),
            (Case::Acc, "en"),
        ] {
            let surface = format!("{lemma}{suffix}");
            let features = Features {
                number: Some(Number::Sg),
                case: Some(case),
                pron_type: Some(PronType::Ind),
                ..Features::empty()
            };
            out.push((
                surface,
                Analysis::with_source(lemma, UPOS::PRON, features, Source::Attested),
            ));
        }
        // Bare-stem Acc (colloquial): "jemand" without -en ending.
        out.push((
            lemma.to_string(),
            Analysis::with_source(
                lemma,
                UPOS::PRON,
                Features {
                    number: Some(Number::Sg),
                    case: Some(Case::Acc),
                    ..Features::empty()
                },
                Source::Attested,
            ),
        ));
    }

    // man — 3Sg, Nom only. The oblique cases use suppletive `einen`
    // (Acc) and `einem` (Dat) — those surfaces are already in the
    // lexicon as forms of `ein`, so we don't duplicate them here.
    out.push((
        "man".to_string(),
        Analysis::with_source(
            "man",
            UPOS::PRON,
            Features {
                person: Some(Person::P3),
                number: Some(Number::Sg),
                case: Some(Case::Nom),
                ..Features::empty()
            },
            Source::Attested,
        ),
    ));

    // etwas, nichts — invariant.
    for &lemma in &["etwas", "nichts"] {
        out.push((
            lemma.to_string(),
            Analysis::with_source(lemma, UPOS::PRON, Features::empty(), Source::Attested),
        ));
    }

    // alle, aller, allen, alles — quantifier with strong-adjective endings.
    let all_forms: &[(&str, Number, Option<Gender>, Case)] = &[
        ("alle", Number::Pl, None, Case::Nom),
        ("aller", Number::Pl, None, Case::Gen),
        ("allen", Number::Pl, None, Case::Dat),
        ("alle", Number::Pl, None, Case::Acc),
        ("alles", Number::Sg, Some(Gender::Neut), Case::Nom),
        ("alles", Number::Sg, Some(Gender::Neut), Case::Acc),
    ];
    for &(surface, number, gender, case) in all_forms {
        out.push((
            surface.to_string(),
            Analysis::with_source(
                "all",
                UPOS::PRON,
                Features {
                    number: Some(number),
                    gender,
                    case: Some(case),
                    ..Features::empty()
                },
                Source::Attested,
            ),
        ));
    }

    // Plural-only quantifiers with strong-adjective Pl endings.
    // einige, viele, mehrere, wenige
    for &quant_stem in &["einig", "viel", "mehrer", "wenig"] {
        for &(case, suffix) in &[
            (Case::Nom, "e"),
            (Case::Gen, "er"),
            (Case::Dat, "en"),
            (Case::Acc, "e"),
        ] {
            let surface = format!("{quant_stem}{suffix}");
            let lemma = format!("{quant_stem}e"); // lemma is Nom Pl form
            out.push((
                surface,
                Analysis::with_source(
                    &lemma,
                    UPOS::PRON,
                    Features {
                        number: Some(Number::Pl),
                        case: Some(case),
                        ..Features::empty()
                    },
                    Source::Attested,
                ),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Numerals (cardinal numbers 0-20 plus tens and powers of ten)
// ---------------------------------------------------------------------------

const CARDINAL_NUMERALS: &[&str] = &[
    "null",
    "eins",
    "zwei",
    "drei",
    "vier",
    "fünf",
    "sechs",
    "sieben",
    "acht",
    "neun",
    "zehn",
    "elf",
    "zwölf",
    "dreizehn",
    "vierzehn",
    "fünfzehn",
    "sechzehn",
    "siebzehn",
    "achtzehn",
    "neunzehn",
    "zwanzig",
    "dreißig",
    "vierzig",
    "fünfzig",
    "sechzig",
    "siebzig",
    "achtzig",
    "neunzig",
    "hundert",
    "tausend",
    "Million",
    "Milliarde",
    "Billion",
];

fn add_numerals(out: &mut Vec<ClosedClassEntry>) {
    for &numeral in CARDINAL_NUMERALS {
        out.push((
            numeral.to_string(),
            Analysis::with_source(numeral, UPOS::NUM, Features::empty(), Source::Attested),
        ));
    }
    // "ein" as numeral (alongside the article); declensions already
    // covered under `add_ein_pattern`, but the bare lemma "eins" /
    // "ein" needs an explicit Num entry too.
    out.push((
        "ein".to_string(),
        Analysis::with_source("ein", UPOS::NUM, Features::empty(), Source::Attested),
    ));
}

// ---------------------------------------------------------------------------
// Ordinal numerals (erster, zweiter, dritter, ...)
// ---------------------------------------------------------------------------

/// Ordinal lemmas (Nom Sg Weak masc form, used as the citation form in
/// Wiktionary). Each ordinal declines like a regular adjective; we
/// reuse the adjective paradigm generator and retag the output as
/// `UPOS::NUM`.
const ORDINAL_LEMMAS: &[&str] = &[
    "erste",
    "zweite",
    "dritte",
    "vierte",
    "fünfte",
    "sechste",
    "siebte",
    "achte",
    "neunte",
    "zehnte",
    "elfte",
    "zwölfte",
    "dreizehnte",
    "vierzehnte",
    "fünfzehnte",
    "sechzehnte",
    "siebzehnte",
    "achtzehnte",
    "neunzehnte",
    "zwanzigste",
    "dreißigste",
    "vierzigste",
    "fünfzigste",
    "sechzigste",
    "siebzigste",
    "achtzigste",
    "neunzigste",
    "hundertste",
    "tausendste",
];

fn add_ordinals(out: &mut Vec<ClosedClassEntry>) {
    use crate::paradigm::adjective::{generate_adjective_paradigm, AdjectiveAttested};
    for &lemma in ORDINAL_LEMMAS {
        let inputs = AdjectiveAttested {
            lemma,
            komparativ: None,
            superlativ: None,
        };
        for (surface, mut analysis) in generate_adjective_paradigm(&inputs) {
            // Retag as numeral; keep the rest of the analysis intact
            // (degree=UPOS, declension/case/number/gender on attributive
            // cells, etc.). Also retag as Lexicon since the ordinal
            // table itself is hand-curated by us.
            analysis.pos = UPOS::NUM;
            analysis.source = Source::Attested;
            out.push((surface, analysis));
        }
    }
}

// ---------------------------------------------------------------------------
// Compound cardinal numerals (21-99, 200-900, 2000-9000)
// ---------------------------------------------------------------------------

/// Single-digit forms that compose into the 21-99 / hundreds / thousands
/// shapes. Note "ein" (not "eins") is the stem inside compounds: 21 is
/// "einundzwanzig", not "einsundzwanzig".
const ONES_COMPOUND_STEMS: &[&str] = &[
    "ein", "zwei", "drei", "vier", "fünf", "sechs", "sieben", "acht", "neun",
];

/// Tens-bases that combine with `ones + "und"` to form 21-99.
const TENS_BASES: &[&str] = &[
    "zwanzig", "dreißig", "vierzig", "fünfzig", "sechzig", "siebzig", "achtzig", "neunzig",
];

fn add_compound_cardinals(out: &mut Vec<ClosedClassEntry>) {
    // 21-99: <ones>und<tens>, e.g. einundzwanzig, zweiundzwanzig, ...,
    // neunundneunzig (9 × 8 = 72 entries).
    for &ten in TENS_BASES {
        for &one in ONES_COMPOUND_STEMS {
            let surface = format!("{one}und{ten}");
            out.push((
                surface.clone(),
                Analysis::with_source(&surface, UPOS::NUM, Features::empty(), Source::Attested),
            ));
        }
    }
    // 200-900: <ones>hundert, e.g. zweihundert, dreihundert, ..., neunhundert.
    for &one in ONES_COMPOUND_STEMS {
        if one == "ein" {
            continue; // "einhundert" is sometimes used but the canonical 100 is just "hundert"
        }
        let surface = format!("{one}hundert");
        out.push((
            surface.clone(),
            Analysis::with_source(&surface, UPOS::NUM, Features::empty(), Source::Attested),
        ));
    }
    // 2000-9000: <ones>tausend.
    for &one in ONES_COMPOUND_STEMS {
        if one == "ein" {
            continue;
        }
        let surface = format!("{one}tausend");
        out.push((
            surface.clone(),
            Analysis::with_source(&surface, UPOS::NUM, Features::empty(), Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// Conjunctions
// ---------------------------------------------------------------------------

/// Coordinating conjunctions (UPOS::CCONJ).
const COORDINATING_CONJUNCTIONS: &[&str] = &[
    "und",
    "oder",
    "aber",
    "denn",
    "sondern",
    "doch",
    "sowie",
    "beziehungsweise",
    "bzw.",
];

/// Subordinating conjunctions (UPOS::SCONJ). Multi-word conjunctions
/// (`ohne dass`, `statt dass`, `als ob`) are skipped — they require
/// multi-token analysis, which is the parser's job, not the
/// morphological lexicon's.
const SUBORDINATING_CONJUNCTIONS: &[&str] = &[
    "dass",
    "weil",
    "wenn",
    "als",
    "ob",
    "obwohl",
    "obgleich",
    "obschon",
    "obzwar",
    "da",
    "während",
    "bevor",
    "ehe",
    "nachdem",
    "seitdem",
    "seit",
    "sobald",
    "solange",
    "sooft",
    "falls",
    "sofern",
    "damit",
    "indem",
    "sodass",
    "sodaß",
    "anstatt",
    "wohingegen",
    "wenngleich",
    "zumal",
];

fn add_conjunctions(out: &mut Vec<ClosedClassEntry>) {
    for &c in COORDINATING_CONJUNCTIONS {
        out.push((
            c.to_string(),
            Analysis::with_source(c, UPOS::CCONJ, Features::empty(), Source::Attested),
        ));
    }
    for &c in SUBORDINATING_CONJUNCTIONS {
        out.push((
            c.to_string(),
            Analysis::with_source(c, UPOS::SCONJ, Features::empty(), Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// Prepositions (UPOS::ADP)
// ---------------------------------------------------------------------------

/// German prepositions, grouped by the case they govern. Cases aren't
/// stored on the prepositions themselves (we'd need a separate
/// "governed case" feature for that); the parser/syntax layer is
/// responsible for inferring case from context.
const PREPOSITIONS: &[&str] = &[
    // Accusative
    "für",
    "durch",
    "gegen",
    "ohne",
    "um",
    "bis",
    "wider",
    "entlang",
    // Dative
    "aus",
    "bei",
    "mit",
    "nach",
    "seit",
    "von",
    "zu",
    "gegenüber",
    "ab",
    "außer",
    "binnen",
    "samt",
    "nebst",
    "dank",
    // Wechselpräpositionen (Acc OR Dat depending on motion/state)
    "in",
    "an",
    "auf",
    "unter",
    "über",
    "hinter",
    "vor",
    "neben",
    "zwischen",
    // Genitive
    "während",
    "trotz",
    "wegen",
    "statt",
    "anstatt",
    "mittels",
    "kraft",
    "laut",
    "infolge",
    "anlässlich",
    "oberhalb",
    "unterhalb",
    "jenseits",
    "diesseits",
    "innerhalb",
    "außerhalb",
    "angesichts",
    "aufgrund",
    "zwecks",
    "betreffs",
    "hinsichtlich",
    "ungeachtet",
    // Common contractions written as separate "prepositions" in many
    // analyses (am = an + dem, im = in + dem, zum = zu + dem, etc.)
    "am",
    "im",
    "zum",
    "zur",
    "ans",
    "ins",
    "vom",
    "beim",
    "aufs",
    "fürs",
];

fn add_prepositions(out: &mut Vec<ClosedClassEntry>) {
    for &p in PREPOSITIONS {
        out.push((
            p.to_string(),
            Analysis::with_source(p, UPOS::ADP, Features::empty(), Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// Punctuation (UPOS::PUNCT)
// ---------------------------------------------------------------------------

/// Standard punctuation marks emitted in modern German orthography
/// plus the common typographic variants. UD conventionally uses
/// `lemma == surface` for punctuation tokens, so the lemma here is
/// the surface itself.
///
/// Coverage includes:
/// - Sentence terminators (. ! ? …)
/// - Clause separators (, ; :)
/// - Brackets (( ) [ ] { } ‹ › « »)
/// - German typographic quotation marks („ " ‚ ' « »)
/// - Dashes / hyphens (- – —)
/// - Misc (/ \ * & @ # % § ° ' ´ ` " ¿ ¡)
const PUNCTUATION: &[&str] = &[
    // Sentence terminators
    ".", "!", "?", "…", // Clause separators
    ",", ";", ":", // Brackets
    "(", ")", "[", "]", "{", "}", "‹", "›", "«", "»",
    // Quotation marks (German + common ASCII)
    "„", "\"", "‚", "'", "‟", "‛", "‘", "’", "“", "”", // Dashes and hyphens
    "-", "–", "—", "‐", "‑", // Misc punctuation common in German text
    "/", "\\", "*", "&", "@", "#", "%", "§", "°", "´", "`", "¿", "¡",
    // Multi-character ellipsis variants
    "...",
];

fn add_punctuation(out: &mut Vec<ClosedClassEntry>) {
    for &p in PUNCTUATION {
        out.push((
            p.to_string(),
            Analysis::with_source(p, UPOS::PUNCT, Features::empty(), Source::Attested),
        ));
    }
}

// ---------------------------------------------------------------------------
// Demonstrative / quantifying determiners (dieser / jener / jeder / ...)
// ---------------------------------------------------------------------------

/// Singular endings for the demonstrative pattern (strong-adjective set).
const DEMONSTRATIVE_PATTERN_SG: &[(Case, Gender, &str)] = &[
    (Case::Nom, Gender::Masc, "er"),
    (Case::Nom, Gender::Fem, "e"),
    (Case::Nom, Gender::Neut, "es"),
    (Case::Gen, Gender::Masc, "es"),
    (Case::Gen, Gender::Fem, "er"),
    (Case::Gen, Gender::Neut, "es"),
    (Case::Dat, Gender::Masc, "em"),
    (Case::Dat, Gender::Fem, "er"),
    (Case::Dat, Gender::Neut, "em"),
    (Case::Acc, Gender::Masc, "en"),
    (Case::Acc, Gender::Fem, "e"),
    (Case::Acc, Gender::Neut, "es"),
];

const DEMONSTRATIVE_PATTERN_PL: &[(Case, &str)] = &[
    (Case::Nom, "e"),
    (Case::Gen, "er"),
    (Case::Dat, "en"),
    (Case::Acc, "e"),
];

/// Add the full paradigm of a demonstrative-style determiner.
///
/// The input `lemma` is the Masc-Sg-Nom form (which by convention is
/// also the citation form: `dieser`, `jener`, …). The stem is the
/// lemma with its trailing `-er` stripped.
fn add_demonstrative_pattern(out: &mut Vec<ClosedClassEntry>, lemma: &str) {
    let stem = match lemma.strip_suffix("er") {
        Some(s) => s,
        // Defensive fallback: if the lemma doesn't end in -er,
        // use it verbatim as the stem.
        None => lemma,
    };
    for &(case, gender, suffix) in DEMONSTRATIVE_PATTERN_SG {
        let surface = format!("{stem}{suffix}");
        let features = Features {
            number: Some(Number::Sg),
            case: Some(case),
            gender: Some(gender),
            ..Features::empty()
        };
        out.push((
            surface,
            Analysis::with_source(lemma, UPOS::DET, features, Source::Attested),
        ));
    }
    for &(case, suffix) in DEMONSTRATIVE_PATTERN_PL {
        let surface = format!("{stem}{suffix}");
        let features = Features {
            number: Some(Number::Pl),
            case: Some(case),
            ..Features::empty()
        };
        out.push((
            surface,
            Analysis::with_source(lemma, UPOS::DET, features, Source::Attested),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_surfaces<'a>(
        entries: &'a [ClosedClassEntry],
        pos: UPOS,
        lemma: &str,
        case: Case,
        number: Number,
        gender: Option<Gender>,
    ) -> Vec<&'a str> {
        entries
            .iter()
            .filter(|(_, a)| {
                a.pos == pos
                    && a.lemma == lemma
                    && a.features.case == Some(case)
                    && a.features.number == Some(number)
                    && a.features.gender == gender
            })
            .map(|(s, _)| s.as_str())
            .collect()
    }

    #[test]
    fn personal_pronouns_1sg() {
        let entries = generate_closed_class_entries();
        // 1Sg paradigm of "ich".
        for (case, expected) in [
            (Case::Nom, "ich"),
            (Case::Acc, "mich"),
            (Case::Dat, "mir"),
            (Case::Gen, "meiner"),
        ] {
            let s = find_surfaces(&entries, UPOS::PRON, "ich", case, Number::Sg, None);
            assert_eq!(s, vec![expected], "1Sg {case:?}");
        }
    }

    #[test]
    fn personal_pronouns_3sg_per_gender() {
        let entries = generate_closed_class_entries();
        // er/sie/es Nom Sg.
        assert_eq!(
            find_surfaces(
                &entries,
                UPOS::PRON,
                "er",
                Case::Nom,
                Number::Sg,
                Some(Gender::Masc)
            ),
            vec!["er"]
        );
        assert_eq!(
            find_surfaces(
                &entries,
                UPOS::PRON,
                "sie",
                Case::Nom,
                Number::Sg,
                Some(Gender::Fem)
            ),
            vec!["sie"]
        );
        assert_eq!(
            find_surfaces(
                &entries,
                UPOS::PRON,
                "es",
                Case::Nom,
                Number::Sg,
                Some(Gender::Neut)
            ),
            vec!["es"]
        );
        // Accusative er → ihn.
        assert_eq!(
            find_surfaces(
                &entries,
                UPOS::PRON,
                "er",
                Case::Acc,
                Number::Sg,
                Some(Gender::Masc)
            ),
            vec!["ihn"]
        );
        // Dative er → ihm; sie → ihr.
        assert_eq!(
            find_surfaces(
                &entries,
                UPOS::PRON,
                "er",
                Case::Dat,
                Number::Sg,
                Some(Gender::Masc)
            ),
            vec!["ihm"]
        );
        assert_eq!(
            find_surfaces(
                &entries,
                UPOS::PRON,
                "sie",
                Case::Dat,
                Number::Sg,
                Some(Gender::Fem)
            ),
            vec!["ihr"]
        );
    }

    #[test]
    fn definite_article_paradigm() {
        let entries = generate_closed_class_entries();
        let der =
            |case, number, gender| find_surfaces(&entries, UPOS::DET, "der", case, number, gender);
        assert_eq!(der(Case::Nom, Number::Sg, Some(Gender::Masc)), vec!["der"]);
        assert_eq!(der(Case::Nom, Number::Sg, Some(Gender::Fem)), vec!["die"]);
        assert_eq!(der(Case::Nom, Number::Sg, Some(Gender::Neut)), vec!["das"]);
        assert_eq!(der(Case::Gen, Number::Sg, Some(Gender::Masc)), vec!["des"]);
        assert_eq!(der(Case::Dat, Number::Pl, None), vec!["den"]);
    }

    #[test]
    fn ein_paradigm_no_plural() {
        let entries = generate_closed_class_entries();
        let ein_pl_count = entries
            .iter()
            .filter(|(_, a)| a.lemma == "ein" && a.features.number == Some(Number::Pl))
            .count();
        assert_eq!(ein_pl_count, 0, "indefinite article should have no plural");

        // Singular Masc Nom: "ein" (bare).
        let s = find_surfaces(
            &entries,
            UPOS::DET,
            "ein",
            Case::Nom,
            Number::Sg,
            Some(Gender::Masc),
        );
        assert_eq!(s, vec!["ein"]);
        // Singular Masc Acc: "einen".
        let s = find_surfaces(
            &entries,
            UPOS::DET,
            "ein",
            Case::Acc,
            Number::Sg,
            Some(Gender::Masc),
        );
        assert_eq!(s, vec!["einen"]);
    }

    #[test]
    fn kein_paradigm_has_plural() {
        let entries = generate_closed_class_entries();
        let s = find_surfaces(&entries, UPOS::DET, "kein", Case::Dat, Number::Pl, None);
        assert_eq!(s, vec!["keinen"]);
        let s = find_surfaces(
            &entries,
            UPOS::DET,
            "kein",
            Case::Nom,
            Number::Sg,
            Some(Gender::Fem),
        );
        assert_eq!(s, vec!["keine"]);
    }

    #[test]
    fn all_entries_tagged_lexicon() {
        let entries = generate_closed_class_entries();
        for (_, a) in &entries {
            assert_eq!(a.source, Source::Attested);
        }
    }

    #[test]
    fn possessive_mein_paradigm() {
        let entries = generate_closed_class_entries();
        // Sg Masc Nom: bare "mein".
        let s = find_surfaces(
            &entries,
            UPOS::DET,
            "mein",
            Case::Nom,
            Number::Sg,
            Some(Gender::Masc),
        );
        assert_eq!(s, vec!["mein"]);
        // Sg Masc Acc: "meinen".
        let s = find_surfaces(
            &entries,
            UPOS::DET,
            "mein",
            Case::Acc,
            Number::Sg,
            Some(Gender::Masc),
        );
        assert_eq!(s, vec!["meinen"]);
        // Sg Fem Gen: "meiner".
        let s = find_surfaces(
            &entries,
            UPOS::DET,
            "mein",
            Case::Gen,
            Number::Sg,
            Some(Gender::Fem),
        );
        assert_eq!(s, vec!["meiner"]);
        // Pl Dat: "meinen".
        let s = find_surfaces(&entries, UPOS::DET, "mein", Case::Dat, Number::Pl, None);
        assert_eq!(s, vec!["meinen"]);
    }

    #[test]
    fn possessive_unser_paradigm() {
        let entries = generate_closed_class_entries();
        // Masc Nom Sg: bare "unser" (no reduction since there's no
        // suffix to swallow the medial -e-).
        let s = find_surfaces(
            &entries,
            UPOS::DET,
            "unser",
            Case::Nom,
            Number::Sg,
            Some(Gender::Masc),
        );
        assert_eq!(s, vec!["unser"]);
        // Fem Nom Sg: both "unsere" (standard) and "unsre" (vowel-reduced
        // colloquial variant) are emitted.
        let s = find_surfaces(
            &entries,
            UPOS::DET,
            "unser",
            Case::Nom,
            Number::Sg,
            Some(Gender::Fem),
        );
        assert!(s.contains(&"unsere"), "missing standard 'unsere' in {s:?}");
        assert!(s.contains(&"unsre"), "missing reduced 'unsre' in {s:?}");
    }

    #[test]
    fn possessive_euer_vowel_reduction() {
        let entries = generate_closed_class_entries();
        // Fem Nom Sg: both "euere" and "eure" should be emitted.
        let s = find_surfaces(
            &entries,
            UPOS::DET,
            "euer",
            Case::Nom,
            Number::Sg,
            Some(Gender::Fem),
        );
        assert!(s.contains(&"euere"), "missing standard 'euere' in {s:?}");
        assert!(s.contains(&"eure"), "missing reduced 'eure' in {s:?}");
    }

    #[test]
    fn demonstrative_dieser_paradigm() {
        let entries = generate_closed_class_entries();
        let dieser = |case, number, gender| {
            find_surfaces(&entries, UPOS::DET, "dieser", case, number, gender)
        };
        assert_eq!(
            dieser(Case::Nom, Number::Sg, Some(Gender::Masc)),
            vec!["dieser"]
        );
        assert_eq!(
            dieser(Case::Nom, Number::Sg, Some(Gender::Fem)),
            vec!["diese"]
        );
        assert_eq!(
            dieser(Case::Nom, Number::Sg, Some(Gender::Neut)),
            vec!["dieses"]
        );
        assert_eq!(
            dieser(Case::Dat, Number::Sg, Some(Gender::Masc)),
            vec!["diesem"]
        );
        assert_eq!(dieser(Case::Dat, Number::Pl, None), vec!["diesen"]);
    }

    #[test]
    fn interrogative_wer_paradigm() {
        let entries = generate_closed_class_entries();
        let wer = |case| {
            entries
                .iter()
                .filter(|(_, a)| {
                    a.pos == UPOS::PRON && a.lemma == "wer" && a.features.case == Some(case)
                })
                .map(|(s, _)| s.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(wer(Case::Nom), vec!["wer"]);
        assert_eq!(wer(Case::Gen), vec!["wessen"]);
        assert_eq!(wer(Case::Dat), vec!["wem"]);
        assert_eq!(wer(Case::Acc), vec!["wen"]);
    }

    #[test]
    fn interrogative_was_paradigm() {
        let entries = generate_closed_class_entries();
        let was = |case| {
            entries
                .iter()
                .filter(|(_, a)| {
                    a.pos == UPOS::PRON && a.lemma == "was" && a.features.case == Some(case)
                })
                .map(|(s, _)| s.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(was(Case::Nom), vec!["was"]);
        assert_eq!(was(Case::Gen), vec!["wessen"]);
        assert_eq!(was(Case::Acc), vec!["was"]);
        // No Dat in standard German for "was"; prepositional "wo-" forms it.
        assert!(was(Case::Dat).is_empty());
    }

    #[test]
    fn wessen_has_two_lemma_analyses() {
        // "wessen" is the Gen form of both `wer` and `was`. Both
        // analyses should be present so the analyzer can return both
        // readings.
        let entries = generate_closed_class_entries();
        let lemmas: Vec<&str> = entries
            .iter()
            .filter(|(s, _)| s == "wessen")
            .map(|(_, a)| &*a.lemma)
            .collect();
        assert!(lemmas.contains(&"wer"), "missing wer for wessen");
        assert!(lemmas.contains(&"was"), "missing was for wessen");
    }

    #[test]
    fn reflexive_3sg_uses_sich_lemma() {
        let entries = generate_closed_class_entries();
        let sich_entries: Vec<_> = entries
            .iter()
            .filter(|(s, a)| s == "sich" && a.lemma == "sich")
            .collect();
        // 3Sg Acc/Dat + 3Pl Acc/Dat = 4 entries.
        assert_eq!(sich_entries.len(), 4, "{sich_entries:#?}");
    }

    #[test]
    fn reflexive_shares_surface_with_personal() {
        // "mich" is both 1Sg Acc personal (lemma=ich) and reflexive (lemma=sich).
        let entries = generate_closed_class_entries();
        let mich_lemmas: Vec<&str> = entries
            .iter()
            .filter(|(s, _)| s == "mich")
            .map(|(_, a)| &*a.lemma)
            .collect();
        assert!(mich_lemmas.contains(&"ich"));
        assert!(mich_lemmas.contains(&"sich"));
    }

    #[test]
    fn relative_dessen_and_deren() {
        // The relative-specific Genitive forms differ from the
        // definite article's `des` / `der`.
        let entries = generate_closed_class_entries();
        let rel_gen: Vec<(&str, &str)> = entries
            .iter()
            .filter(|(_, a)| {
                a.pos == UPOS::PRON && a.lemma == "der" && a.features.case == Some(Case::Gen)
            })
            .map(|(s, a)| (s.as_str(), a.features.gender.map(|_| "Sg").unwrap_or("Pl")))
            .collect();
        assert!(
            rel_gen.iter().any(|(s, _)| *s == "dessen"),
            "missing dessen in {rel_gen:?}"
        );
        assert!(
            rel_gen.iter().any(|(s, _)| *s == "deren"),
            "missing deren in {rel_gen:?}"
        );
    }

    #[test]
    fn indefinite_jemand_paradigm() {
        let entries = generate_closed_class_entries();
        let jemand: Vec<&str> = entries
            .iter()
            .filter(|(_, a)| a.lemma == "jemand")
            .map(|(s, _)| s.as_str())
            .collect();
        assert!(jemand.contains(&"jemand"));
        assert!(jemand.contains(&"jemandes"));
        assert!(jemand.contains(&"jemandem"));
        assert!(jemand.contains(&"jemanden"));
    }

    #[test]
    fn indefinite_man_only_nom() {
        let entries = generate_closed_class_entries();
        let man: Vec<_> = entries.iter().filter(|(_, a)| a.lemma == "man").collect();
        assert_eq!(man.len(), 1);
        assert_eq!(man[0].1.features.case, Some(Case::Nom));
    }

    #[test]
    fn cardinal_numerals_emitted() {
        let entries = generate_closed_class_entries();
        let nums: Vec<&str> = entries
            .iter()
            .filter(|(_, a)| a.pos == UPOS::NUM)
            .map(|(s, _)| s.as_str())
            .collect();
        for expected in &[
            "null", "eins", "zwei", "drei", "zehn", "zwanzig", "hundert", "tausend",
        ] {
            assert!(nums.contains(expected), "missing numeral {expected}");
        }
    }

    #[test]
    fn coordinating_conjunctions_emitted() {
        let entries = generate_closed_class_entries();
        let cc: Vec<&str> = entries
            .iter()
            .filter(|(_, a)| a.pos == UPOS::CCONJ)
            .map(|(s, _)| s.as_str())
            .collect();
        for expected in &["und", "oder", "aber", "denn"] {
            assert!(cc.contains(expected), "missing Cconj {expected}");
        }
    }

    #[test]
    fn subordinating_conjunctions_emitted() {
        let entries = generate_closed_class_entries();
        let sc: Vec<&str> = entries
            .iter()
            .filter(|(_, a)| a.pos == UPOS::SCONJ)
            .map(|(s, _)| s.as_str())
            .collect();
        for expected in &["dass", "weil", "wenn", "obwohl"] {
            assert!(sc.contains(expected), "missing Sconj {expected}");
        }
    }

    #[test]
    fn common_prepositions_emitted() {
        let entries = generate_closed_class_entries();
        let preps: Vec<&str> = entries
            .iter()
            .filter(|(_, a)| a.pos == UPOS::ADP)
            .map(|(s, _)| s.as_str())
            .collect();
        for expected in &["in", "auf", "mit", "von", "zu", "für", "ohne", "während"] {
            assert!(preps.contains(expected), "missing Adp {expected}");
        }
    }

    #[test]
    fn demonstrative_welcher_paradigm() {
        let entries = generate_closed_class_entries();
        let welcher = |case, number, gender| {
            find_surfaces(&entries, UPOS::DET, "welcher", case, number, gender)
        };
        assert_eq!(
            welcher(Case::Nom, Number::Sg, Some(Gender::Masc)),
            vec!["welcher"]
        );
        assert_eq!(
            welcher(Case::Acc, Number::Sg, Some(Gender::Masc)),
            vec!["welchen"]
        );
        assert_eq!(welcher(Case::Nom, Number::Pl, None), vec!["welche"]);
    }
}
