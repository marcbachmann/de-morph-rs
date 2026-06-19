//! German verb paradigm rules.
//!
//! Takes the small set of forms that Wiktionary actually stores (1/2/3
//! Sg Pres Ind, 1 Sg Past Ind, 1 Sg Konj II, Imp Sg, Imp Pl, Partizip
//! II) and produces the full synthetic paradigm:
//!
//! - 6 Pres Ind cells (Person × Number)
//! - 6 Past Ind cells
//! - 6 Konj I cells (Sub1, Pres)
//! - 6 Konj II cells (Sub2, Past)
//! - 2 Imp cells (Sg / Pl, 2nd person)
//! - Inf, InfZu, PtcPres, PtcPerf
//!
//! Forms attested by Wiktionary keep `Source::Lexicon`; rule-derived
//! cells are `Source::Generated`. When the rule's output happens to
//! coincide with an attested form (a common case), both are emitted —
//! that's OK because the downstream FST builder will collapse
//! duplicates while keeping multiple analyses per surface.
//!
//! Out-of-scope for v0 (documented limitations):
//! - Separable-prefix verbs ("anfangen") with split conjugation.
//! - Reflexive verbs.
//! - True suppletives (sein, haben, werden, tun): the rule-derived
//!   plural Pres Ind forms will be wrong for these. A small
//!   closed-class override table is the planned follow-up; for now
//!   the wrong forms are emitted and tagged `Source::Generated` so a
//!   later lookup-vs-guess discriminator can identify them.
//!
//! References (verified): widely-attested rules of German verb
//! conjugation; any modern German reference grammar carries them
//! (Duden Grammatik, Helbig & Buscha). No specific section is cited
//! because the maintainer did not consult a copy while writing this
//! file.

use crate::analysis::{Analysis, Features, Mood, Number, Person, UPOS, Source, Tense, VerbForm};

/// The Wiktionary-attested forms for one verb, as parsed from the
/// `{{Deutsch Verb Übersicht}}` template.
#[derive(Debug, Clone, Default)]
pub struct VerbAttested<'a> {
    /// Page title; serves as the infinitive (citation form).
    pub infinitive: &'a str,
    pub present_1sg: Option<&'a str>,
    pub present_2sg: Option<&'a str>,
    pub present_3sg: Option<&'a str>,
    pub past_1sg: Option<&'a str>,
    pub konj_ii_1sg: Option<&'a str>,
    pub imperativ_sg: Option<&'a str>,
    pub imperativ_pl: Option<&'a str>,
    pub partizip_perf: Option<&'a str>,
}

/// One paradigm cell: surface form paired with its analysis.
pub type VerbCell = (String, Analysis);

/// Generate the full synthetic paradigm for one verb.
///
/// Returns ~30 cells when all attested inputs are present; fewer when
/// some Wiktionary fields are missing (the rule layer falls back to
/// what it can derive from the infinitive alone).
pub fn generate_verb_paradigm(inputs: &VerbAttested) -> Vec<VerbCell> {
    // Separable verbs (abtauchen = ab + tauchen): Wiktionary stores the
    // SEPARATED finite forms ("tauche ab"). Conjugate the base verb and
    // join the prefix, emitting the single-token forms a token-based
    // analyzer needs (abtauche, abzutauchen, abtauchend) rather than the
    // unusable separated "tauche ab" or the wrong "brach ausst".
    if let Some((prefix, base)) = split_separable(inputs) {
        let base_cells = generate_verb_paradigm(&base);
        return join_separable(prefix, base.infinitive, inputs.infinitive, base_cells);
    }

    let mut out = Vec::with_capacity(32);
    let inf = inputs.infinitive;
    let stem = infinitive_stem(inf);

    // --- Non-finite forms -------------------------------------------------
    push(&mut out, inf, inf, Source::Generated, features_form(VerbForm::Inf));
    push(
        &mut out,
        &format!("zu {inf}"),
        inf,
        Source::Generated,
        features_form(VerbForm::InfZu),
    );
    push(
        &mut out,
        &format!("{inf}d"),
        inf,
        Source::Generated,
        features_form(VerbForm::PtcPres),
    );
    if let Some(p2) = inputs.partizip_perf {
        push(&mut out, p2, inf, Source::Lexicon, features_form(VerbForm::PtcPerf));
    }

    // --- Present Indikativ ------------------------------------------------
    if let Some(s) = inputs.present_1sg {
        push(
            &mut out,
            s,
            inf,
            Source::Lexicon,
            features_finite(Person::P1, Number::Sg, Tense::Pres, Mood::Ind),
        );
    }
    if let Some(s) = inputs.present_2sg {
        push(
            &mut out,
            s,
            inf,
            Source::Lexicon,
            features_finite(Person::P2, Number::Sg, Tense::Pres, Mood::Ind),
        );
    }
    if let Some(s) = inputs.present_3sg {
        push(
            &mut out,
            s,
            inf,
            Source::Lexicon,
            features_finite(Person::P3, Number::Sg, Tense::Pres, Mood::Ind),
        );
    }
    // Plurals: 1Pl = 3Pl = infinitive (regular rule); 2Pl = stem + (e)t.
    push(
        &mut out,
        inf,
        inf,
        Source::Generated,
        features_finite(Person::P1, Number::Pl, Tense::Pres, Mood::Ind),
    );
    push(
        &mut out,
        &present_2pl(&stem),
        inf,
        Source::Generated,
        features_finite(Person::P2, Number::Pl, Tense::Pres, Mood::Ind),
    );
    push(
        &mut out,
        inf,
        inf,
        Source::Generated,
        features_finite(Person::P3, Number::Pl, Tense::Pres, Mood::Ind),
    );

    // --- Präteritum Indikativ --------------------------------------------
    if let Some(p1) = inputs.past_1sg {
        for (person, number, form, attested) in past_paradigm(p1) {
            let source = if attested {
                Source::Lexicon
            } else {
                Source::Generated
            };
            push(
                &mut out,
                &form,
                inf,
                source,
                features_finite(person, number, Tense::Past, Mood::Ind),
            );
        }
    }

    // --- Konjunktiv I (Präsens) ------------------------------------------
    // Konj I uses the infinitive stem + Konj-I endings (-e, -est, -e,
    // -en, -et, -en). For irregular verbs (sein → ich sei), this rule
    // produces wrong forms; the closed-class override TODO covers it.
    for (person, number, form) in konj_i_paradigm(&stem) {
        push(
            &mut out,
            &form,
            inf,
            Source::Generated,
            features_finite(person, number, Tense::Pres, Mood::Sub1),
        );
    }

    // --- Konjunktiv II ---------------------------------------------------
    if let Some(k1) = inputs.konj_ii_1sg {
        for (person, number, form, attested) in past_paradigm(k1) {
            let source = if attested {
                Source::Lexicon
            } else {
                Source::Generated
            };
            push(
                &mut out,
                &form,
                inf,
                source,
                features_finite(person, number, Tense::Past, Mood::Sub2),
            );
        }
    }

    // --- Imperativ -------------------------------------------------------
    if let Some(s) = inputs.imperativ_sg {
        push(
            &mut out,
            s,
            inf,
            Source::Lexicon,
            features_imperativ(Number::Sg),
        );
    }
    if let Some(s) = inputs.imperativ_pl {
        push(
            &mut out,
            s,
            inf,
            Source::Lexicon,
            features_imperativ(Number::Pl),
        );
    }

    // --- Suppletive overrides --------------------------------------------
    // For a handful of highly irregular verbs (sein, haben, werden, …)
    // the rule-derived plural Pres Ind / Konj I / PtcPres forms are
    // wrong. The override table below replaces those cells with curated
    // surfaces. Each override is tagged Source::Lexicon since the
    // surface is hand-attested by the maintainer.
    apply_suppletive_overrides(inf, &mut out);

    out
}

/// One override rule: replace any output cell whose features match
/// `matches_features` with `(surface, Source::Lexicon)`.
struct OverrideCell {
    /// Optional feature matchers; `None` means "match anything for this slot".
    person: Option<Person>,
    number: Option<Number>,
    tense: Option<Tense>,
    mood: Option<Mood>,
    form: Option<VerbForm>,
    /// The correct surface for this cell.
    surface: &'static str,
}

impl OverrideCell {
    const fn pres_ind(p: Person, n: Number, surface: &'static str) -> Self {
        Self {
            person: Some(p),
            number: Some(n),
            tense: Some(Tense::Pres),
            mood: Some(Mood::Ind),
            form: Some(VerbForm::Fin),
            surface,
        }
    }
    const fn konj_i(p: Person, n: Number, surface: &'static str) -> Self {
        Self {
            person: Some(p),
            number: Some(n),
            tense: Some(Tense::Pres),
            mood: Some(Mood::Sub1),
            form: Some(VerbForm::Fin),
            surface,
        }
    }
    const fn ptc_pres(surface: &'static str) -> Self {
        Self {
            person: None,
            number: None,
            tense: None,
            mood: None,
            form: Some(VerbForm::PtcPres),
            surface,
        }
    }

    fn matches(&self, f: &Features) -> bool {
        let p_ok = self.person.map_or(true, |p| f.person == Some(p));
        let n_ok = self.number.map_or(true, |n| f.number == Some(n));
        let t_ok = self.tense.map_or(true, |t| f.tense == Some(t));
        let m_ok = self.mood.map_or(true, |m| f.mood == Some(m));
        let fm_ok = self.form.map_or(true, |fm| f.form == Some(fm));
        p_ok && n_ok && t_ok && m_ok && fm_ok
    }

    fn target_features(&self) -> Features {
        Features {
            person: self.person,
            number: self.number,
            tense: self.tense,
            mood: self.mood,
            form: self.form,
            ..Features::empty()
        }
    }
}

/// Per-lemma suppletive override table.
///
/// References: high-frequency German auxiliaries and modal verbs whose
/// rule-derived forms are wrong. Surfaces below are the standard
/// modern-German forms (any reference grammar carries the same).
const SUPPLETIVE_OVERRIDES: &[(&str, &[OverrideCell])] = &[
    (
        "sein",
        &[
            // Pres Ind plural: completely irregular ("sind" / "seid" / "sind").
            OverrideCell::pres_ind(Person::P1, Number::Pl, "sind"),
            OverrideCell::pres_ind(Person::P2, Number::Pl, "seid"),
            OverrideCell::pres_ind(Person::P3, Number::Pl, "sind"),
            // Konj I 1Sg / 3Sg: "sei" (without -e ending).
            OverrideCell::konj_i(Person::P1, Number::Sg, "sei"),
            OverrideCell::konj_i(Person::P3, Number::Sg, "sei"),
            // Konj I plural: rule yields "seien"/"seiet"/"seien" — correct
            // (override-equivalent — listed here for documentation).
            // PtcPres: "seiend" (rule would yield "seind").
            OverrideCell::ptc_pres("seiend"),
        ],
    ),
    // `haben` and `werden` are already handled correctly by the rule
    // layer once their attested Wiktionary fields are in place
    // (`habst`/`hast`, `wirst`/`wird`, etc. are stored). No override
    // entries needed for v0.
    (
        "tun",
        &[
            // The only other non-`-en` infinitive: `PtcPres = inf + "d"`
            // yields the invalid "tund"; the correct participle is
            // "tuend". Present plural (wir/sie tun, ihr tut), Konj I
            // (tue/tuest/…) and the past (tat-) are all derived
            // correctly by the rule layer, so only PtcPres needs fixing.
            OverrideCell::ptc_pres("tuend"),
        ],
    ),
];

fn apply_suppletive_overrides(infinitive: &str, out: &mut Vec<VerbCell>) {
    let overrides = match SUPPLETIVE_OVERRIDES
        .iter()
        .find(|(lemma, _)| *lemma == infinitive)
    {
        Some((_, ov)) => *ov,
        None => return,
    };
    for ov in overrides {
        // Remove rule-derived cells that would conflict.
        out.retain(|(_, a)| !ov.matches(&a.features));
        // Insert the override cell, tagged Lexicon (hand-curated).
        let analysis = Analysis::with_source(
            infinitive,
            UPOS::VERB,
            ov.target_features(),
            Source::Lexicon,
        );
        out.push((ov.surface.to_string(), analysis));
    }
}

// =========================================================================
// Internals
// =========================================================================

/// If `inputs` describes a separable verb — the attested finite forms
/// carry a separated prefix particle (present "tauche ab" for the
/// infinitive "abtauchen") — split it into the prefix and a base
/// [`VerbAttested`] with the particle removed. Returns `None` for
/// ordinary (non-separable, inseparable-prefix) verbs.
///
/// Detection is data-driven: the particle is the trailing token of a
/// separated finite form, and it must be a literal prefix of the
/// infinitive. This avoids guessing from a prefix list and never
/// misfires on inseparable prefixes (be-, ver-, ent-, …), whose forms
/// are stored joined and contain no space.
fn split_separable<'a>(inputs: &VerbAttested<'a>) -> Option<(&'a str, VerbAttested<'a>)> {
    let candidates = [
        inputs.present_3sg,
        inputs.present_1sg,
        inputs.present_2sg,
        inputs.past_1sg,
        inputs.imperativ_sg,
    ];
    // A separated finite form is "base particle[ particle…]" — the base
    // (first token) is the conjugated verb, and everything after it is the
    // separated prefix particle(s): "tauche ab" → "ab", "stelle wieder her"
    // → "wieder her". Concatenated in order the particles form a literal
    // prefix of the infinitive (abtauchen, wiederherstellen). This also
    // covers double-prefix verbs and never misfires on inseparable
    // prefixes (be-/ver-/ent-, whose forms carry no space) or on multiword
    // lemmas (whose infinitive itself contains spaces, so the space-free
    // joined prefix can't be a prefix of it).
    let rest = candidates
        .into_iter()
        .flatten()
        .find_map(|f| f.split_once(' ').map(|(_, rest)| rest))?;
    let inf = inputs.infinitive;
    let joined: String = rest.split_whitespace().collect();
    if joined.is_empty() || !inf.starts_with(&joined) {
        return None;
    }
    let base_inf = &inf[joined.len()..];
    if base_inf.len() < 2 || !(base_inf.ends_with("en") || base_inf.ends_with('n')) {
        return None;
    }
    let prefix = &inf[..joined.len()];

    // Strip the separated particles: keep only the base (first token).
    let strip_fin = |f: Option<&'a str>| -> Option<&'a str> {
        let s = f?;
        Some(s.split_once(' ').map(|(base, _)| base).unwrap_or(s))
    };
    // Partizip II is already joined (abgetaucht); strip the LEADING prefix.
    let base_partizip = inputs
        .partizip_perf
        .map(|p| p.strip_prefix(prefix).unwrap_or(p));

    let base = VerbAttested {
        infinitive: base_inf,
        present_1sg: strip_fin(inputs.present_1sg),
        present_2sg: strip_fin(inputs.present_2sg),
        present_3sg: strip_fin(inputs.present_3sg),
        past_1sg: strip_fin(inputs.past_1sg),
        konj_ii_1sg: strip_fin(inputs.konj_ii_1sg),
        // Separable imperatives are inherently two-token ("tauch ab") —
        // skip them rather than emit the non-word "abtauch".
        imperativ_sg: None,
        imperativ_pl: None,
        partizip_perf: base_partizip,
    };
    Some((prefix, base))
}

/// Re-attach the separable prefix to every base-paradigm cell, producing
/// single-token surfaces and re-lemmatising to the separable infinitive.
/// The zu-infinitive is special: `zu` goes BETWEEN prefix and base
/// (`abzutauchen`), not in front (`zu abtauchen`).
fn join_separable(
    prefix: &str,
    base_inf: &str,
    separable_inf: &str,
    base_cells: Vec<VerbCell>,
) -> Vec<VerbCell> {
    base_cells
        .into_iter()
        .map(|(surface, mut analysis)| {
            let joined = if analysis.features.form == Some(VerbForm::InfZu) {
                format!("{prefix}zu{base_inf}")
            } else {
                format!("{prefix}{surface}")
            };
            analysis.lemma = separable_inf.to_string();
            (joined, analysis)
        })
        .collect()
}

#[inline]
fn push(
    out: &mut Vec<VerbCell>,
    surface: &str,
    lemma: &str,
    source: Source,
    features: Features,
) {
    let analysis = Analysis::with_source(lemma, UPOS::VERB, features, source);
    out.push((surface.to_string(), analysis));
}

#[inline]
fn features_finite(person: Person, number: Number, tense: Tense, mood: Mood) -> Features {
    Features {
        person: Some(person),
        number: Some(number),
        tense: Some(tense),
        mood: Some(mood),
        form: Some(VerbForm::Fin),
        ..Features::empty()
    }
}

#[inline]
fn features_imperativ(number: Number) -> Features {
    Features {
        person: Some(Person::P2),
        number: Some(number),
        mood: Some(Mood::Imp),
        form: Some(VerbForm::Fin),
        ..Features::empty()
    }
}

#[inline]
fn features_form(form: VerbForm) -> Features {
    Features {
        form: Some(form),
        ..Features::empty()
    }
}

/// Extract the verb stem from the infinitive.
///
/// Rules (in order):
///   - infinitive ends in `-en` → strip `en` (lieben → lieb, geben → geb)
///   - infinitive ends in `-n`  → strip `n`  (wandern → wander, lächeln → lächel)
///   - otherwise                → infinitive as-is (fallback for truly
///     irregular shapes — caller should override)
fn infinitive_stem(inf: &str) -> String {
    if let Some(s) = inf.strip_suffix("en") {
        s.to_string()
    } else if let Some(s) = inf.strip_suffix('n') {
        s.to_string()
    } else {
        inf.to_string()
    }
}

/// 2nd-person-plural Pres Ind form: stem + t, with epenthetic -e- if
/// the stem ends in `t` or `d` (you need a vowel for pronounceability:
/// "ihr arbeitet", not "ihr arbeitt").
fn present_2pl(stem: &str) -> String {
    if needs_e_link(stem) {
        format!("{stem}et")
    } else {
        format!("{stem}t")
    }
}

/// Build the six (Person, Number, surface, attested) tuples for past
/// indicative, given the 1Sg past form. The 4th element is `true` for
/// the 1Sg cell (which IS the attested input) and `false` for the
/// other five derived cells.
fn past_paradigm(past_1sg: &str) -> Vec<(Person, Number, String, bool)> {
    let stem = past_1sg;
    let ends_in_e = stem.ends_with('e');
    let e_link = needs_e_link(stem) && !ends_in_e;

    // The 2Sg -st ending needs an epenthetic -e- after BOTH t/d stems
    // (du rittest) AND sibilant stems s/ß/z/x/sch (du aßest, du lasest),
    // since *aßst / *lasst are unpronounceable. The 2Pl -t ending only
    // needs it after t/d (du wartetest → ihr wartetet; but ihr aßt, no
    // epenthesis), so `ihr` below keeps using `e_link`.
    let du = if !ends_in_e && (needs_e_link(stem) || ends_in_sibilant(stem)) {
        format!("{stem}est")
    } else {
        format!("{stem}st")
    };
    let er = stem.to_string();
    let wir = if ends_in_e {
        format!("{stem}n")
    } else {
        format!("{stem}en")
    };
    let ihr = if e_link {
        format!("{stem}et")
    } else if ends_in_e {
        format!("{stem}t")
    } else {
        format!("{stem}t")
    };
    let sie = wir.clone();

    vec![
        (Person::P1, Number::Sg, stem.to_string(), true),
        (Person::P2, Number::Sg, du, false),
        (Person::P3, Number::Sg, er, false),
        (Person::P1, Number::Pl, wir, false),
        (Person::P2, Number::Pl, ihr, false),
        (Person::P3, Number::Pl, sie, false),
    ]
}

/// Build the six Konjunktiv-I Präsens cells from the infinitive stem.
///
/// Endings: -e / -est / -e / -en / -et / -en. Notably:
///   - 1Sg Konj I = 1Sg Pres Ind (same form for regular verbs);
///   - 3Sg Konj I differs from 3Sg Pres Ind (er liebe vs er liebt);
///   - 2Pl Konj I differs from 2Pl Pres Ind (ihr liebet vs ihr liebt).
fn konj_i_paradigm(stem: &str) -> Vec<(Person, Number, String)> {
    vec![
        (Person::P1, Number::Sg, format!("{stem}e")),
        (Person::P2, Number::Sg, format!("{stem}est")),
        (Person::P3, Number::Sg, format!("{stem}e")),
        (Person::P1, Number::Pl, format!("{stem}en")),
        (Person::P2, Number::Pl, format!("{stem}et")),
        (Person::P3, Number::Pl, format!("{stem}en")),
    ]
}

/// Whether the stem requires an epenthetic -e- before consonant
/// endings (-st, -t). True for stems ending in `t` or `d`.
fn needs_e_link(stem: &str) -> bool {
    let last = stem.chars().last().unwrap_or(' ');
    matches!(last, 't' | 'd')
}

/// Whether the stem ends in a sibilant (`s`, `ß`, `z`, `x`, or `sch`).
/// Such stems need an epenthetic -e- before a 2Sg `-st` ending
/// (`aß` → `aßest`, `las` → `lasest`), because the bare `-st` would
/// fuse the sibilants unpronounceably (`*aßst`).
fn ends_in_sibilant(stem: &str) -> bool {
    stem.ends_with('s')
        || stem.ends_with('ß')
        || stem.ends_with('z')
        || stem.ends_with('x')
        || stem.ends_with("sch")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lieben_inputs() -> VerbAttested<'static> {
        VerbAttested {
            infinitive: "lieben",
            present_1sg: Some("liebe"),
            present_2sg: Some("liebst"),
            present_3sg: Some("liebt"),
            past_1sg: Some("liebte"),
            konj_ii_1sg: Some("liebte"),
            imperativ_sg: Some("liebe"),
            imperativ_pl: Some("liebt"),
            partizip_perf: Some("geliebt"),
        }
    }

    fn find(
        cells: &[VerbCell],
        person: Option<Person>,
        number: Option<Number>,
        tense: Option<Tense>,
        mood: Option<Mood>,
        form: Option<VerbForm>,
    ) -> Vec<(String, Source)> {
        cells
            .iter()
            .filter(|(_, a)| {
                a.features.person == person
                    && a.features.number == number
                    && a.features.tense == tense
                    && a.features.mood == mood
                    && a.features.form == form
            })
            .map(|(s, a)| (s.clone(), a.source))
            .collect()
    }

    #[test]
    fn lieben_full_paradigm_size() {
        let cells = generate_verb_paradigm(&lieben_inputs());
        // Inf + InfZu + PtcPres + PtcPerf            = 4
        // Pres Ind ×6                                = 6
        // Past Ind ×6                                = 6
        // Konj I  ×6                                 = 6
        // Konj II ×6                                 = 6
        // Imp Sg + Imp Pl                            = 2
        // Total                                      = 30
        assert_eq!(cells.len(), 30, "{cells:#?}");
    }

    #[test]
    fn lieben_inf_and_partizipien() {
        let cells = generate_verb_paradigm(&lieben_inputs());
        assert_eq!(
            find(&cells, None, None, None, None, Some(VerbForm::Inf)),
            vec![("lieben".into(), Source::Generated)]
        );
        assert_eq!(
            find(&cells, None, None, None, None, Some(VerbForm::InfZu)),
            vec![("zu lieben".into(), Source::Generated)]
        );
        assert_eq!(
            find(&cells, None, None, None, None, Some(VerbForm::PtcPres)),
            vec![("liebend".into(), Source::Generated)]
        );
        assert_eq!(
            find(&cells, None, None, None, None, Some(VerbForm::PtcPerf)),
            vec![("geliebt".into(), Source::Lexicon)]
        );
    }

    #[test]
    fn lieben_present_indikativ_all_six_persons() {
        let cells = generate_verb_paradigm(&lieben_inputs());
        let pi = |p, n| {
            find(
                &cells,
                Some(p),
                Some(n),
                Some(Tense::Pres),
                Some(Mood::Ind),
                Some(VerbForm::Fin),
            )
        };
        assert_eq!(pi(Person::P1, Number::Sg), vec![("liebe".into(), Source::Lexicon)]);
        assert_eq!(pi(Person::P2, Number::Sg), vec![("liebst".into(), Source::Lexicon)]);
        assert_eq!(pi(Person::P3, Number::Sg), vec![("liebt".into(), Source::Lexicon)]);
        assert_eq!(pi(Person::P1, Number::Pl), vec![("lieben".into(), Source::Generated)]);
        assert_eq!(pi(Person::P2, Number::Pl), vec![("liebt".into(), Source::Generated)]);
        assert_eq!(pi(Person::P3, Number::Pl), vec![("lieben".into(), Source::Generated)]);
    }

    #[test]
    fn lieben_past_indikativ_derivation() {
        let cells = generate_verb_paradigm(&lieben_inputs());
        let past = |p, n| {
            find(
                &cells,
                Some(p),
                Some(n),
                Some(Tense::Past),
                Some(Mood::Ind),
                Some(VerbForm::Fin),
            )
        };
        // 1Sg attested as "liebte"; 3Sg = 1Sg in German past.
        assert_eq!(past(Person::P1, Number::Sg), vec![("liebte".into(), Source::Lexicon)]);
        assert_eq!(past(Person::P3, Number::Sg), vec![("liebte".into(), Source::Generated)]);
        // 2Sg adds "st" after the -e stem: "liebtest".
        assert_eq!(past(Person::P2, Number::Sg), vec![("liebtest".into(), Source::Generated)]);
        // 1Pl/3Pl: stem ends in -e, so add just -n: "liebten".
        assert_eq!(past(Person::P1, Number::Pl), vec![("liebten".into(), Source::Generated)]);
        assert_eq!(past(Person::P3, Number::Pl), vec![("liebten".into(), Source::Generated)]);
        // 2Pl: "liebtet".
        assert_eq!(past(Person::P2, Number::Pl), vec![("liebtet".into(), Source::Generated)]);
    }

    #[test]
    fn lieben_konj_i_endings() {
        // Konj I has distinctive -et 2Pl and -e 3Sg.
        let cells = generate_verb_paradigm(&lieben_inputs());
        let k1 = |p, n| {
            find(
                &cells,
                Some(p),
                Some(n),
                Some(Tense::Pres),
                Some(Mood::Sub1),
                Some(VerbForm::Fin),
            )
        };
        assert_eq!(k1(Person::P1, Number::Sg), vec![("liebe".into(), Source::Generated)]);
        assert_eq!(k1(Person::P2, Number::Sg), vec![("liebest".into(), Source::Generated)]);
        assert_eq!(k1(Person::P3, Number::Sg), vec![("liebe".into(), Source::Generated)]);
        assert_eq!(k1(Person::P1, Number::Pl), vec![("lieben".into(), Source::Generated)]);
        assert_eq!(k1(Person::P2, Number::Pl), vec![("liebet".into(), Source::Generated)]);
        assert_eq!(k1(Person::P3, Number::Pl), vec![("lieben".into(), Source::Generated)]);
    }

    #[test]
    fn lieben_konj_ii() {
        let cells = generate_verb_paradigm(&lieben_inputs());
        let k2 = |p, n| {
            find(
                &cells,
                Some(p),
                Some(n),
                Some(Tense::Past),
                Some(Mood::Sub2),
                Some(VerbForm::Fin),
            )
        };
        // 1Sg attested as "liebte"; rest derived.
        assert_eq!(k2(Person::P1, Number::Sg), vec![("liebte".into(), Source::Lexicon)]);
        assert_eq!(k2(Person::P2, Number::Sg), vec![("liebtest".into(), Source::Generated)]);
        assert_eq!(k2(Person::P1, Number::Pl), vec![("liebten".into(), Source::Generated)]);
    }

    #[test]
    fn lieben_imperativ_attested() {
        let cells = generate_verb_paradigm(&lieben_inputs());
        let imp = |n| {
            find(
                &cells,
                Some(Person::P2),
                Some(n),
                None,
                Some(Mood::Imp),
                Some(VerbForm::Fin),
            )
        };
        assert_eq!(imp(Number::Sg), vec![("liebe".into(), Source::Lexicon)]);
        assert_eq!(imp(Number::Pl), vec![("liebt".into(), Source::Lexicon)]);
    }

    #[test]
    fn strong_verb_past_derivation() {
        // "singen": ich sang. Past derivation should yield sang/sangst/sang/sangen/sangt/sangen.
        let inputs = VerbAttested {
            infinitive: "singen",
            present_1sg: Some("singe"),
            present_2sg: Some("singst"),
            present_3sg: Some("singt"),
            past_1sg: Some("sang"),
            konj_ii_1sg: Some("sänge"),
            imperativ_sg: Some("sing"),
            imperativ_pl: Some("singt"),
            partizip_perf: Some("gesungen"),
        };
        let cells = generate_verb_paradigm(&inputs);
        let past = |p, n| {
            find(
                &cells,
                Some(p),
                Some(n),
                Some(Tense::Past),
                Some(Mood::Ind),
                Some(VerbForm::Fin),
            )
        };
        assert_eq!(past(Person::P1, Number::Sg), vec![("sang".into(), Source::Lexicon)]);
        assert_eq!(past(Person::P2, Number::Sg), vec![("sangst".into(), Source::Generated)]);
        assert_eq!(past(Person::P3, Number::Sg), vec![("sang".into(), Source::Generated)]);
        assert_eq!(past(Person::P1, Number::Pl), vec![("sangen".into(), Source::Generated)]);
        assert_eq!(past(Person::P2, Number::Pl), vec![("sangt".into(), Source::Generated)]);
        assert_eq!(past(Person::P3, Number::Pl), vec![("sangen".into(), Source::Generated)]);
    }

    #[test]
    fn strong_verb_past_2sg_sibilant_stem_gets_epenthetic_e() {
        // "essen": ich aß. The 2Sg past needs an epenthetic -e- before
        // -st because the stem ends in a sibilant: "du aßest" (not the
        // unpronounceable "aßst"). The 2Pl "ihr aßt" takes NO epenthesis.
        let inputs = VerbAttested {
            infinitive: "essen",
            present_1sg: Some("esse"),
            present_2sg: Some("isst"),
            present_3sg: Some("isst"),
            past_1sg: Some("aß"),
            konj_ii_1sg: Some("äße"),
            imperativ_sg: Some("iss"),
            imperativ_pl: Some("esst"),
            partizip_perf: Some("gegessen"),
        };
        let cells = generate_verb_paradigm(&inputs);
        let past = |p, n| {
            find(
                &cells,
                Some(p),
                Some(n),
                Some(Tense::Past),
                Some(Mood::Ind),
                Some(VerbForm::Fin),
            )
        };
        assert_eq!(past(Person::P2, Number::Sg), vec![("aßest".into(), Source::Generated)]);
        assert_eq!(past(Person::P2, Number::Pl), vec![("aßt".into(), Source::Generated)]);
        assert!(
            !cells.iter().any(|(s, _)| s == "aßst"),
            "over-generated unpronounceable 'aßst'"
        );
    }

    #[test]
    fn warten_e_link_in_present_2pl() {
        // "warten" → stem "wart" → ihr wartet (epenthetic -e- because stem ends in -t).
        let inputs = VerbAttested {
            infinitive: "warten",
            present_1sg: Some("warte"),
            present_2sg: Some("wartest"),
            present_3sg: Some("wartet"),
            past_1sg: Some("wartete"),
            konj_ii_1sg: Some("wartete"),
            imperativ_sg: Some("warte"),
            imperativ_pl: Some("wartet"),
            partizip_perf: Some("gewartet"),
        };
        let cells = generate_verb_paradigm(&inputs);
        let pi_2pl = find(
            &cells,
            Some(Person::P2),
            Some(Number::Pl),
            Some(Tense::Pres),
            Some(Mood::Ind),
            Some(VerbForm::Fin),
        );
        assert_eq!(pi_2pl, vec![("wartet".into(), Source::Generated)]);
    }

    #[test]
    fn sein_suppletive_override_fixes_pres_ind_plural() {
        let inputs = VerbAttested {
            infinitive: "sein",
            present_1sg: Some("bin"),
            present_2sg: Some("bist"),
            present_3sg: Some("ist"),
            past_1sg: Some("war"),
            konj_ii_1sg: Some("wäre"),
            imperativ_sg: Some("sei"),
            imperativ_pl: Some("seid"),
            partizip_perf: Some("gewesen"),
        };
        let cells = generate_verb_paradigm(&inputs);
        let pi = |p, n| {
            find(
                &cells,
                Some(p),
                Some(n),
                Some(Tense::Pres),
                Some(Mood::Ind),
                Some(VerbForm::Fin),
            )
        };
        // Without overrides, the rule would emit "sein" for 1Pl/3Pl
        // and "seint" or similar for 2Pl. The override fixes all three.
        assert_eq!(pi(Person::P1, Number::Pl), vec![("sind".into(), Source::Lexicon)]);
        assert_eq!(pi(Person::P2, Number::Pl), vec![("seid".into(), Source::Lexicon)]);
        assert_eq!(pi(Person::P3, Number::Pl), vec![("sind".into(), Source::Lexicon)]);
    }

    #[test]
    fn sein_suppletive_override_fixes_konj_i_sg() {
        let inputs = VerbAttested {
            infinitive: "sein",
            present_1sg: Some("bin"),
            present_2sg: Some("bist"),
            present_3sg: Some("ist"),
            past_1sg: Some("war"),
            konj_ii_1sg: Some("wäre"),
            imperativ_sg: Some("sei"),
            imperativ_pl: Some("seid"),
            partizip_perf: Some("gewesen"),
        };
        let cells = generate_verb_paradigm(&inputs);
        let k1 = |p, n| {
            find(
                &cells,
                Some(p),
                Some(n),
                Some(Tense::Pres),
                Some(Mood::Sub1),
                Some(VerbForm::Fin),
            )
        };
        // 1Sg and 3Sg Konj I should be "sei", not "seie".
        assert_eq!(k1(Person::P1, Number::Sg), vec![("sei".into(), Source::Lexicon)]);
        assert_eq!(k1(Person::P3, Number::Sg), vec![("sei".into(), Source::Lexicon)]);
    }

    #[test]
    fn sein_suppletive_override_fixes_ptc_pres() {
        let inputs = VerbAttested {
            infinitive: "sein",
            present_1sg: Some("bin"),
            present_2sg: Some("bist"),
            present_3sg: Some("ist"),
            past_1sg: Some("war"),
            konj_ii_1sg: Some("wäre"),
            imperativ_sg: Some("sei"),
            imperativ_pl: Some("seid"),
            partizip_perf: Some("gewesen"),
        };
        let cells = generate_verb_paradigm(&inputs);
        let ptc = cells
            .iter()
            .find(|(_, a)| a.features.form == Some(VerbForm::PtcPres))
            .expect("PtcPres missing");
        // Rule would yield "seind"; override gives "seiend".
        assert_eq!(ptc.0, "seiend");
        assert_eq!(ptc.1.source, Source::Lexicon);
    }

    #[test]
    fn tun_suppletive_override_fixes_ptc_pres() {
        // "tun" is one of only two non-`-en` infinitives (with "sein").
        // The rule `PtcPres = inf + "d"` yields the invalid "tund"; the
        // correct present participle is "tuend".
        let inputs = VerbAttested {
            infinitive: "tun",
            present_1sg: Some("tue"),
            present_2sg: Some("tust"),
            present_3sg: Some("tut"),
            past_1sg: Some("tat"),
            konj_ii_1sg: Some("täte"),
            imperativ_sg: Some("tu"),
            imperativ_pl: Some("tut"),
            partizip_perf: Some("getan"),
        };
        let cells = generate_verb_paradigm(&inputs);
        let ptc = cells
            .iter()
            .find(|(_, a)| a.features.form == Some(VerbForm::PtcPres))
            .expect("PtcPres missing");
        assert_eq!(ptc.0, "tuend");
        assert_eq!(ptc.1.source, Source::Lexicon);
        assert!(
            !cells.iter().any(|(s, _)| s == "tund"),
            "over-generated invalid 'tund'"
        );
    }

    #[test]
    fn non_suppletive_verb_unaffected_by_overrides() {
        // "lieben" is not in the override table — paradigm should be
        // identical to what the rule layer produces.
        let cells = generate_verb_paradigm(&lieben_inputs());
        let pi_1pl = find(
            &cells,
            Some(Person::P1),
            Some(Number::Pl),
            Some(Tense::Pres),
            Some(Mood::Ind),
            Some(VerbForm::Fin),
        );
        assert_eq!(pi_1pl, vec![("lieben".into(), Source::Generated)]);
    }

    #[test]
    fn separable_weak_verb_joins_prefix() {
        // "abtauchen" = ab + tauchen. Wiktionary stores the SEPARATED
        // finite forms ("tauche ab"). We emit the joined single-token
        // forms a token-based analyzer needs: abtauche / abtauchst /
        // abzutauchen / abtauchend, never "tauche ab" or "zu abtauchen".
        let inputs = VerbAttested {
            infinitive: "abtauchen",
            present_1sg: Some("tauche ab"),
            present_2sg: Some("tauchst ab"),
            present_3sg: Some("taucht ab"),
            past_1sg: Some("tauchte ab"),
            konj_ii_1sg: Some("tauchte ab"),
            imperativ_sg: Some("tauch ab"),
            imperativ_pl: Some("taucht ab"),
            partizip_perf: Some("abgetaucht"),
        };
        let cells = generate_verb_paradigm(&inputs);
        assert!(
            cells.iter().all(|(s, _)| !s.contains(' ')),
            "separable paradigm must be single-token, got {cells:#?}"
        );
        assert!(cells.iter().all(|(_, a)| a.lemma == "abtauchen"));
        let has = |q: &str| cells.iter().any(|(s, _)| s == q);
        assert!(has("abtauchen"), "Inf");
        assert!(has("abzutauchen"), "zu-Inf");
        assert!(has("abtauchend"), "PtcPres");
        assert!(has("abgetaucht"), "PtcPerf");
        assert!(has("abtauche"), "1Sg pres");
        assert!(has("abtauchst"), "2Sg pres");
        assert!(has("abtaucht"), "3Sg pres");
        assert!(has("abtauchte"), "1Sg past");
        assert!(has("abtauchtest"), "2Sg past");
        assert!(!has("zu abtauchen"), "wrong zu-inf must be gone");
        // Imperatives are inherently separated (two-token) — not emitted.
        assert!(
            find(&cells, Some(Person::P2), Some(Number::Sg), None, Some(Mood::Imp), Some(VerbForm::Fin)).is_empty()
        );
    }

    #[test]
    fn separable_strong_verb_joins_prefix() {
        // "ausbrechen" = aus + brechen (strong). Previously produced
        // garbage like "brach ausst"; now joins correctly.
        let inputs = VerbAttested {
            infinitive: "ausbrechen",
            present_1sg: Some("breche aus"),
            present_2sg: Some("brichst aus"),
            present_3sg: Some("bricht aus"),
            past_1sg: Some("brach aus"),
            konj_ii_1sg: Some("bräche aus"),
            imperativ_sg: Some("brich aus"),
            imperativ_pl: Some("brecht aus"),
            partizip_perf: Some("ausgebrochen"),
        };
        let cells = generate_verb_paradigm(&inputs);
        let has = |q: &str| cells.iter().any(|(s, _)| s == q);
        assert!(cells.iter().all(|(s, _)| !s.contains(' ')));
        assert!(has("ausbrechen") && has("auszubrechen") && has("ausgebrochen"));
        assert!(has("ausbricht"), "3Sg");
        assert!(has("ausbreche"), "1Sg");
        assert!(has("ausbrach"), "1Sg past");
        assert!(has("ausbrachst"), "2Sg past (ch, no epenthesis)");
        assert!(!has("brach ausst"));
    }

    #[test]
    fn separable_double_prefix_verb() {
        // "wiederherstellen" = wieder + her + stellen. The separated finite
        // forms carry BOTH particles ("stelle wieder her"); concatenated
        // they prefix the infinitive. The zu-infinitive inserts -zu- after
        // the full prefix: "wiederherzustellen".
        let inputs = VerbAttested {
            infinitive: "wiederherstellen",
            present_1sg: Some("stelle wieder her"),
            present_2sg: Some("stellst wieder her"),
            present_3sg: Some("stellt wieder her"),
            past_1sg: Some("stellte wieder her"),
            konj_ii_1sg: Some("stellte wieder her"),
            imperativ_sg: Some("stell wieder her"),
            imperativ_pl: Some("stellt wieder her"),
            partizip_perf: Some("wiederhergestellt"),
        };
        let cells = generate_verb_paradigm(&inputs);
        assert!(
            cells.iter().all(|(s, _)| !s.contains(' ')),
            "must be single-token, got {cells:#?}"
        );
        assert!(cells.iter().all(|(_, a)| a.lemma == "wiederherstellen"));
        let has = |q: &str| cells.iter().any(|(s, _)| s == q);
        assert!(has("wiederherstellen"), "Inf");
        assert!(has("wiederherzustellen"), "zu-Inf");
        assert!(has("wiederherstellend"), "PtcPres");
        assert!(has("wiederhergestellt"), "PtcPerf");
        assert!(has("wiederherstelle"), "1Sg");
        assert!(has("wiederherstellst"), "2Sg");
        assert!(has("wiederherstellte"), "past 1Sg");
    }

    #[test]
    fn separable_sibilant_base_past_2sg_gets_epenthesis() {
        // "ausreißen" = aus + reißen; base past "riss" ends in a sibilant,
        // so 2Sg is "rissest" → joined "ausrissest" (composes with the
        // sibilant-epenthesis rule).
        let inputs = VerbAttested {
            infinitive: "ausreißen",
            present_1sg: Some("reiße aus"),
            present_2sg: Some("reißt aus"),
            present_3sg: Some("reißt aus"),
            past_1sg: Some("riss aus"),
            konj_ii_1sg: Some("risse aus"),
            imperativ_sg: Some("reiß aus"),
            imperativ_pl: Some("reißt aus"),
            partizip_perf: Some("ausgerissen"),
        };
        let cells = generate_verb_paradigm(&inputs);
        let has = |q: &str| cells.iter().any(|(s, _)| s == q);
        assert!(cells.iter().all(|(s, _)| !s.contains(' ')));
        assert!(has("auszureißen") && has("ausgerissen"));
        assert!(has("ausrissest"), "joined sibilant past 2Sg");
    }

    #[test]
    fn infinitive_stem_handles_en_n_and_other() {
        assert_eq!(infinitive_stem("lieben"), "lieb");
        assert_eq!(infinitive_stem("geben"), "geb");
        assert_eq!(infinitive_stem("wandern"), "wander");
        assert_eq!(infinitive_stem("lächeln"), "lächel");
        // Fallback: no -en/-n suffix → return as-is.
        assert_eq!(infinitive_stem("tun"), "tu");
        assert_eq!(infinitive_stem("foo"), "foo");
    }
}
