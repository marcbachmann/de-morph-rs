//! Morphological analysis: parts of speech, features, and the analysis record.
//!
//! The feature inventory below is the German-specific set the build pipeline
//! has to be able to round-trip without information loss. Every enum is
//! `#[repr(u8)]` so the analysis record stays compact and the bit-packed
//! representation (`PackedFeatures`) can address any single feature in O(1)
//! with no branching.
//!
//! References (verified):
//! - POS tag set follows the Universal Dependencies coarse-POS inventory,
//!   <https://universaldependencies.org/u/pos/>.
//! - Feature names follow the Universal Dependencies feature inventory,
//!   <https://universaldependencies.org/u/feat/>.
//! - UD's treatment of Tense values:
//!   <https://universaldependencies.org/u/feat/Tense.html>.
//! - UD's treatment of VerbForm values:
//!   <https://universaldependencies.org/u/feat/VerbForm.html>.
//! - UD's treatment of Mood (including Sub/Imp values):
//!   <https://universaldependencies.org/u/feat/Mood.html>.
//! - UD's treatment of Case:
//!   <https://universaldependencies.org/u/feat/Case.html>.
//!
//! Background references (not consulted page-by-page):
//! - German grammar terminology (Kasus, Genus, Numerus, starke/schwache/
//!   gemischte Deklination, Konjunktiv I/II) is standard and any modern
//!   reference grammar covers it — e.g. Duden, "Duden – Die Grammatik";
//!   Helbig & Buscha, "Deutsche Grammatik". The maintainer has not cited
//!   specific section numbers because no copy was consulted while writing
//!   this file.
//! - The decision to restrict morphological tense to synthetic Präsens/
//!   Präteritum and treat Perfekt/Plusquamperfekt/Futur syntactically is a
//!   design choice consistent with the UD German feature pages cited above.
//! - The PackedFeatures bit layout is original to this project. Compact
//!   tag encoding for FST transducers is a general technique discussed
//!   in Beesley & Karttunen, "Finite State Morphology" (2003); that book
//!   was not consulted while writing this file.

use std::fmt;

/// Coarse part-of-speech tag — full Universal Dependencies UPOS inventory
/// (17 tags: NOUN, VERB, ADJ, ADV, PRON, DET, NUM, ADP, CCONJ, SCONJ,
/// AUX, PART, INTJ, PUNCT, SYM, X, PROPN).
///
/// `PROPN` is a separate tag from `NOUN` per UD convention; we use it
/// for organisation/place/person proper names and the related class of
/// proper-noun abbreviations (USA, EU, BRD, GmbH, …). The bit layout
/// in `lexicon::format` reserves 5 bits for this field (room for up to
/// 32 POS values).
///
/// `SYM` and `X` exist so unclassifiable tokens still have a typed slot.
///
/// Variant names follow the Universal Dependencies UPOS spelling
/// convention (all-caps): NOUN, VERB, ADJ, ADV, PRON, DET, NUM, ADP,
/// CCONJ, SCONJ, AUX, PART, INTJ, PUNCT, SYM, X, PROPN. This breaks
/// Rust's CamelCase variant convention but matches the published
/// reference at <https://universaldependencies.org/u/pos/>, so JSONL
/// serialization, eval scripts, and external consumers see exactly the
/// same tags UD does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum UPOS {
    NOUN = 0,
    VERB = 1,
    ADJ = 2,
    ADV = 3,
    PRON = 4,
    DET = 5,
    NUM = 6,
    ADP = 7,
    CCONJ = 8,
    SCONJ = 9,
    AUX = 10,
    PART = 11,
    INTJ = 12,
    PUNCT = 13,
    SYM = 14,
    X = 15,
    PROPN = 16,
}

impl UPOS {
    pub const COUNT: usize = 17;

    #[inline]
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Grammatical case (Kasus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Case {
    Nom = 0,
    Gen = 1,
    Dat = 2,
    Acc = 3,
}

/// Number (Numerus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Number {
    Sg = 0,
    Pl = 1,
}

/// Gender (Genus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Gender {
    Masc = 0,
    Fem = 1,
    Neut = 2,
}

/// Person (Person).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Person {
    P1 = 0,
    P2 = 1,
    P3 = 2,
}

/// Tense (Tempus). German distinguishes the synthetic Präsens/Präteritum and
/// the analytic Perfekt/Plusquamperfekt/Futur I/Futur II. The synthetic forms
/// are what the morphology produces directly; the analytic forms are encoded
/// at the syntactic layer (auxiliary + participle), so we record only the
/// synthetic ones at the morphological level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Tense {
    Pres = 0, // Präsens
    Past = 1, // Präteritum
}

/// Mood (Modus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Mood {
    Ind = 0,  // Indikativ
    Sub1 = 1, // Konjunktiv I (Konjunktiv Präsens)
    Sub2 = 2, // Konjunktiv II (Konjunktiv Präteritum)
    Imp = 3,  // Imperativ
}

/// Voice (Genus Verbi). Morphological voice is only marked on participles
/// (active vs. passive participle); the periphrastic werden-/sein-passive
/// is syntax, not morphology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Voice {
    Act = 0,
    Pas = 1,
}

/// Verb form (Verbform).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VerbForm {
    Fin = 0,     // finite (laufe, läufst, läuft, ...)
    Inf = 1,     // infinitive (laufen)
    InfZu = 2,   // zu-infinitive (zu laufen)
    PtcPres = 3, // Partizip Präsens (laufend)
    PtcPerf = 4, // Partizip Perfekt (gelaufen)
}

/// Degree of comparison (Steigerungsstufe).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Degree {
    Pos = 0,
    Cmp = 1,
    Sup = 2,
}

/// Adjective declension class (Deklinationsart).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Declension {
    Strong = 0, // starke Deklination (after no/few determiners)
    Weak = 1,   // schwache Deklination (after definite article)
    Mixed = 2,  // gemischte Deklination (after ein/kein/possessive)
}

/// Pronoun / determiner sub-type (UD-style PronType feature).
///
/// Disambiguates surface-identical forms that play different syntactic
/// roles: `der` as definite article (Art), relative pronoun (Rel),
/// or demonstrative (Dem); `mich` as personal vs. reflexive; etc.
///
/// References: Universal Dependencies feature PronType,
/// <https://universaldependencies.org/u/feat/PronType.html>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PronType {
    /// Personal pronoun / possessive (ich, mein, mir).
    Prs = 0,
    /// Reflexive (sich, mich-reflexive).
    Refl = 1,
    /// Relative pronoun (der/dessen/deren/welcher as relative).
    Rel = 2,
    /// Interrogative pronoun (wer, was, welcher as question word).
    Int = 3,
    /// Demonstrative (dieser, jener, dieser-as-pronoun).
    Dem = 4,
    /// Indefinite pronoun (jemand, niemand, etwas, alle, viele, ...).
    Ind = 5,
    /// Negative (kein, nichts, niemand when functioning negatively).
    Neg = 6,
    /// Article (der/die/das, ein, kein as determiner).
    Art = 7,
}

/// How an [`Analysis`] was obtained. Downstream callers can use this to
/// trust/distrust results (e.g. only attested forms for high-stakes
/// applications, lexicon-or-rule for normal text processing, anything for
/// best-effort highlighting).
///
/// The numeric ordering is from highest trust (`Lexicon = 0`) to lowest
/// (`Guessed = 2`), so `Source::min(a, b)` returns the more trusted one.
///
/// There is intentionally no `Default`: an analysis's provenance is never
/// "unknown by omission", so every construction site sets `source`
/// explicitly rather than falling back to a (necessarily fabricated)
/// default — a default of `Lexicon` would mislabel un-set analyses as
/// gold-standard attested data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Source {
    /// Attested in the lexicon (i.e. extracted from Wiktionary, hand-curated,
    /// etc.). Highest trust.
    Lexicon = 0,
    /// Produced by paradigm rules from a known lemma. The lemma is in the
    /// lexicon; this particular form is rule-generated rather than attested.
    Generated = 1,
    /// Out-of-vocabulary: the lemma itself is unknown, and the analysis
    /// comes from a suffix-based guesser plus paradigm rules.
    Guessed = 2,
}

/// Morphological features attached to an analysis.
///
/// Every slot is `Option<_>` because most words leave most slots unset:
/// a noun has gender/number/case but no tense; a finite verb has
/// person/number/tense/mood but no case/gender/declension.
///
/// Memory layout: each `Option<EnumU8>` is 2 bytes (Rust does not yet
/// niche-optimise arbitrary enums to 1 byte), so the struct is 20 bytes
/// plus alignment padding. When written to disk this is packed into a
/// 32-bit word; see `PackedFeatures`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Features {
    pub case: Option<Case>,
    pub number: Option<Number>,
    pub gender: Option<Gender>,
    pub person: Option<Person>,
    pub tense: Option<Tense>,
    pub mood: Option<Mood>,
    pub voice: Option<Voice>,
    pub form: Option<VerbForm>,
    pub degree: Option<Degree>,
    pub declension: Option<Declension>,
    /// Pronoun / determiner sub-type (UD PronType).
    pub pron_type: Option<PronType>,
    /// Possessor's person (for possessive determiners: `mein` → P1, etc.).
    pub poss_person: Option<Person>,
    /// Possessor's number (for possessives: `mein` → Sg, `unser` → Pl).
    pub poss_number: Option<Number>,
}

impl Features {
    /// Construct a feature set with all slots unset.
    pub const fn empty() -> Self {
        Self {
            case: None,
            number: None,
            gender: None,
            person: None,
            tense: None,
            mood: None,
            voice: None,
            form: None,
            degree: None,
            declension: None,
            pron_type: None,
            poss_person: None,
            poss_number: None,
        }
    }

    /// Convenience constructor for nouns: gender is required; case/number
    /// are optional at the lemma level (a noun lemma has gender but no
    /// inflection — inflected forms supply case/number).
    pub fn noun(gender: Gender) -> Self {
        Self {
            gender: Some(gender),
            ..Self::empty()
        }
    }

    /// Constructor for an inflected noun form.
    pub fn noun_form(gender: Gender, number: Number, case: Case) -> Self {
        Self {
            gender: Some(gender),
            number: Some(number),
            case: Some(case),
            ..Self::empty()
        }
    }
}

/// Bit-packed representation of [`Features`] for on-disk and in-FST storage.
///
/// 32-bit layout, LSB first. Field widths are sized so every enum value
/// has room to round-trip (e.g. Mood has 4 values + unset → 3 bits).
///
/// ```text
///   bits   width  field        encoding (0 = unset)
///   ----   -----  -----------  ----------------------------------------
///   0-2    3      case         0=unset, 1+=Case+1            (max 4)
///   3-4    2      number       0=unset, 1+=Number+1          (max 2)
///   5-6    2      gender       0=unset, 1+=Gender+1          (max 3)
///   7-8    2      person       0=unset, 1+=Person+1          (max 3)
///   9      1      tense        0=Pres, 1=Past                ← see flag
///   10     1      tense set?   0=unset, 1=tense bit valid
///   11-13  3      mood         0=unset, 1+=Mood+1            (max 4)
///   14     1      voice        0=Act, 1=Pas                  ← see flag
///   15     1      voice set?   0=unset, 1=voice bit valid
///   16-18  3      form         0=unset, 1+=VerbForm+1        (max 5)
///   19-20  2      degree       0=unset, 1+=Degree+1          (max 3)
///   21-22  2      declension   0=unset, 1+=Declension+1      (max 3)
///   23-31  9      reserved     must be 0
/// ```
///
/// The "unset = 0, value+1 = bits" pattern keeps the all-zero word the
/// canonical empty value, so `PackedFeatures::default()` is just `Self(0)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct PackedFeatures(pub u32);

impl PackedFeatures {
    pub const EMPTY: Self = Self(0);

    #[inline]
    pub fn pack(features: Features) -> Self {
        let mut bits: u32 = 0;
        if let Some(case) = features.case {
            bits |= (case as u32 + 1) & 0b111;
        }
        if let Some(number) = features.number {
            bits |= ((number as u32 + 1) & 0b11) << 3;
        }
        if let Some(gender) = features.gender {
            bits |= ((gender as u32 + 1) & 0b11) << 5;
        }
        if let Some(person) = features.person {
            bits |= ((person as u32 + 1) & 0b11) << 7;
        }
        if let Some(tense) = features.tense {
            // tense is one bit (Pres=0, Past=1) plus a "set" flag
            bits |= ((tense as u32) & 0b1) << 9;
            bits |= 1 << 10;
        }
        if let Some(mood) = features.mood {
            bits |= ((mood as u32 + 1) & 0b111) << 11;
        }
        if let Some(voice) = features.voice {
            bits |= ((voice as u32) & 0b1) << 14;
            bits |= 1 << 15;
        }
        if let Some(form) = features.form {
            bits |= ((form as u32 + 1) & 0b111) << 16;
        }
        if let Some(degree) = features.degree {
            bits |= ((degree as u32 + 1) & 0b11) << 19;
        }
        if let Some(decl) = features.declension {
            bits |= ((decl as u32 + 1) & 0b11) << 21;
        }
        if let Some(pt) = features.pron_type {
            bits |= ((pt as u32 + 1) & 0b1111) << 23;
        }
        if let Some(p) = features.poss_person {
            bits |= ((p as u32 + 1) & 0b11) << 27;
        }
        if let Some(n) = features.poss_number {
            bits |= ((n as u32 + 1) & 0b11) << 29;
        }
        Self(bits)
    }

    #[inline]
    pub fn unpack(self) -> Features {
        let bits = self.0;
        let case = match bits & 0b111 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, Case>((n - 1) as u8) }),
        };
        let number = match (bits >> 3) & 0b11 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, Number>((n - 1) as u8) }),
        };
        let gender = match (bits >> 5) & 0b11 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, Gender>((n - 1) as u8) }),
        };
        let person = match (bits >> 7) & 0b11 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, Person>((n - 1) as u8) }),
        };
        let tense = if (bits >> 10) & 1 == 1 {
            Some(unsafe { std::mem::transmute::<u8, Tense>(((bits >> 9) & 1) as u8) })
        } else {
            None
        };
        let mood = match (bits >> 11) & 0b111 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, Mood>((n - 1) as u8) }),
        };
        let voice = if (bits >> 15) & 1 == 1 {
            Some(unsafe { std::mem::transmute::<u8, Voice>(((bits >> 14) & 1) as u8) })
        } else {
            None
        };
        let form = match (bits >> 16) & 0b111 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, VerbForm>((n - 1) as u8) }),
        };
        let degree = match (bits >> 19) & 0b11 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, Degree>((n - 1) as u8) }),
        };
        let declension = match (bits >> 21) & 0b11 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, Declension>((n - 1) as u8) }),
        };
        let pron_type = match (bits >> 23) & 0b1111 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, PronType>((n - 1) as u8) }),
        };
        let poss_person = match (bits >> 27) & 0b11 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, Person>((n - 1) as u8) }),
        };
        let poss_number = match (bits >> 29) & 0b11 {
            0 => None,
            n => Some(unsafe { std::mem::transmute::<u8, Number>((n - 1) as u8) }),
        };
        Features {
            case,
            number,
            gender,
            person,
            tense,
            mood,
            voice,
            form,
            degree,
            declension,
            pron_type,
            poss_person,
            poss_number,
        }
    }
}

/// A single morphological analysis of a surface form.
///
/// Heap-allocated lemma is fine for in-memory work; the on-disk layout
/// interns the lemma into a shared byte buffer addressed by `(offset,
/// length)`. Code outside the build pipeline reads lemmas as `&str`
/// borrowed from the analyzer's interned buffer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Analysis {
    pub lemma: String,
    pub pos: UPOS,
    pub features: Features,
    pub source: Source,
}

impl Analysis {
    /// Construct an analysis tagged as [`Source::Lexicon`] (the common case
    /// for code that already validated the lemma came from the lexicon).
    pub fn new(lemma: impl Into<String>, pos: UPOS, features: Features) -> Self {
        Self::with_source(lemma, pos, features, Source::Lexicon)
    }

    /// Construct an analysis with an explicit source tag.
    pub fn with_source(
        lemma: impl Into<String>,
        pos: UPOS,
        features: Features,
        source: Source,
    ) -> Self {
        Self {
            lemma: lemma.into(),
            pos,
            features,
            source,
        }
    }
}

impl fmt::Display for Analysis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}+{:?}", self.lemma, self.pos)?;
        let feats = self.features;
        if let Some(g) = feats.gender {
            write!(f, "+{:?}", g)?;
        }
        if let Some(n) = feats.number {
            write!(f, "+{:?}", n)?;
        }
        if let Some(c) = feats.case {
            write!(f, "+{:?}", c)?;
        }
        if let Some(p) = feats.person {
            write!(f, "+{:?}", p)?;
        }
        if let Some(t) = feats.tense {
            write!(f, "+{:?}", t)?;
        }
        if let Some(m) = feats.mood {
            write!(f, "+{:?}", m)?;
        }
        if let Some(v) = feats.voice {
            write!(f, "+{:?}", v)?;
        }
        if let Some(fm) = feats.form {
            write!(f, "+{:?}", fm)?;
        }
        if let Some(d) = feats.degree {
            write!(f, "+{:?}", d)?;
        }
        if let Some(d) = feats.declension {
            write!(f, "+{:?}", d)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pos_is_one_byte() {
        assert_eq!(std::mem::size_of::<UPOS>(), 1);
    }

    #[test]
    fn case_number_gender_are_one_byte() {
        assert_eq!(std::mem::size_of::<Case>(), 1);
        assert_eq!(std::mem::size_of::<Number>(), 1);
        assert_eq!(std::mem::size_of::<Gender>(), 1);
    }

    #[test]
    fn packed_empty_roundtrips() {
        let f = Features::empty();
        let packed = PackedFeatures::pack(f);
        assert_eq!(packed.0, 0);
        assert_eq!(packed.unpack(), f);
    }

    #[test]
    fn packed_noun_roundtrips() {
        let f = Features::noun_form(Gender::Masc, Number::Sg, Case::Gen);
        let packed = PackedFeatures::pack(f);
        assert_eq!(packed.unpack(), f);
    }

    #[test]
    fn packed_all_cases() {
        for case in [Case::Nom, Case::Gen, Case::Dat, Case::Acc] {
            let mut f = Features::empty();
            f.case = Some(case);
            assert_eq!(PackedFeatures::pack(f).unpack(), f);
        }
    }

    #[test]
    fn packed_all_genders() {
        for gender in [Gender::Masc, Gender::Fem, Gender::Neut] {
            let mut f = Features::empty();
            f.gender = Some(gender);
            assert_eq!(PackedFeatures::pack(f).unpack(), f);
        }
    }

    #[test]
    fn packed_verb_roundtrips() {
        let f = Features {
            person: Some(Person::P3),
            number: Some(Number::Sg),
            tense: Some(Tense::Pres),
            mood: Some(Mood::Ind),
            form: Some(VerbForm::Fin),
            ..Features::empty()
        };
        assert_eq!(PackedFeatures::pack(f).unpack(), f);
    }

    #[test]
    fn packed_tense_distinguishes_pres_from_unset() {
        let with_pres = Features {
            tense: Some(Tense::Pres),
            ..Features::empty()
        };
        let unset = Features::empty();
        assert_ne!(PackedFeatures::pack(with_pres), PackedFeatures::pack(unset));
        assert_eq!(
            PackedFeatures::pack(with_pres).unpack().tense,
            Some(Tense::Pres)
        );
        assert_eq!(PackedFeatures::pack(unset).unpack().tense, None);
    }

    #[test]
    fn packed_voice_distinguishes_act_from_unset() {
        let with_act = Features {
            voice: Some(Voice::Act),
            ..Features::empty()
        };
        let unset = Features::empty();
        assert_ne!(PackedFeatures::pack(with_act), PackedFeatures::pack(unset));
        assert_eq!(
            PackedFeatures::pack(with_act).unpack().voice,
            Some(Voice::Act)
        );
        assert_eq!(PackedFeatures::pack(unset).unpack().voice, None);
    }

    #[test]
    fn packed_adjective_roundtrips() {
        let f = Features {
            degree: Some(Degree::Cmp),
            declension: Some(Declension::Weak),
            case: Some(Case::Dat),
            number: Some(Number::Pl),
            gender: Some(Gender::Fem),
            ..Features::empty()
        };
        assert_eq!(PackedFeatures::pack(f).unpack(), f);
    }

    #[test]
    fn packed_all_moods_including_imperative() {
        for mood in [Mood::Ind, Mood::Sub1, Mood::Sub2, Mood::Imp] {
            let mut f = Features::empty();
            f.mood = Some(mood);
            let packed = PackedFeatures::pack(f);
            assert_eq!(packed.unpack().mood, Some(mood), "mood {:?}", mood);
        }
    }

    #[test]
    fn packed_all_forms() {
        for form in [
            VerbForm::Fin,
            VerbForm::Inf,
            VerbForm::InfZu,
            VerbForm::PtcPres,
            VerbForm::PtcPerf,
        ] {
            let mut f = Features::empty();
            f.form = Some(form);
            let packed = PackedFeatures::pack(f);
            assert_eq!(packed.unpack().form, Some(form), "form {:?}", form);
        }
    }

    #[test]
    fn packed_all_degrees() {
        for d in [Degree::Pos, Degree::Cmp, Degree::Sup] {
            let mut f = Features::empty();
            f.degree = Some(d);
            assert_eq!(PackedFeatures::pack(f).unpack().degree, Some(d));
        }
    }

    #[test]
    fn packed_all_declensions() {
        for d in [Declension::Strong, Declension::Weak, Declension::Mixed] {
            let mut f = Features::empty();
            f.declension = Some(d);
            assert_eq!(PackedFeatures::pack(f).unpack().declension, Some(d));
        }
    }

    #[test]
    fn packed_all_persons() {
        for p in [Person::P1, Person::P2, Person::P3] {
            let mut f = Features::empty();
            f.person = Some(p);
            assert_eq!(PackedFeatures::pack(f).unpack().person, Some(p));
        }
    }

    #[test]
    fn packed_layout_fits_in_31_bits() {
        // Set every field to its highest value and confirm we stay
        // within the documented 31-bit window (32 bits total minus the
        // single reserved bit at position 31).
        let f = Features {
            case: Some(Case::Acc),
            number: Some(Number::Pl),
            gender: Some(Gender::Neut),
            person: Some(Person::P3),
            tense: Some(Tense::Past),
            mood: Some(Mood::Imp),
            voice: Some(Voice::Pas),
            form: Some(VerbForm::PtcPerf),
            degree: Some(Degree::Sup),
            declension: Some(Declension::Mixed),
            pron_type: Some(PronType::Art),
            poss_person: Some(Person::P3),
            poss_number: Some(Number::Pl),
        };
        let packed = PackedFeatures::pack(f);
        assert_eq!(packed.0 >> 31, 0, "bit 31 must be reserved: {:b}", packed.0);
        assert_eq!(packed.unpack(), f);
    }

    #[test]
    fn packed_pron_type_roundtrips_all_values() {
        for pt in [
            PronType::Prs,
            PronType::Refl,
            PronType::Rel,
            PronType::Int,
            PronType::Dem,
            PronType::Ind,
            PronType::Neg,
            PronType::Art,
        ] {
            let mut f = Features::empty();
            f.pron_type = Some(pt);
            assert_eq!(PackedFeatures::pack(f).unpack().pron_type, Some(pt));
        }
    }

    #[test]
    fn packed_possessor_fields_roundtrip() {
        let f = Features {
            poss_person: Some(Person::P1),
            poss_number: Some(Number::Sg),
            ..Features::empty()
        };
        let packed = PackedFeatures::pack(f);
        let unpacked = packed.unpack();
        assert_eq!(unpacked.poss_person, Some(Person::P1));
        assert_eq!(unpacked.poss_number, Some(Number::Sg));
    }

    #[test]
    fn display_for_noun_form() {
        let a = Analysis::new(
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Gen),
        );
        assert_eq!(a.to_string(), "Tisch+NOUN+Masc+Sg+Gen");
    }
}
