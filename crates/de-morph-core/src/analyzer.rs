//! Convenience analyzer wrapping a [`Lexicon`] plus optional OOV
//! fallback via the paradigm guesser.
//!
//! For low-level access, use [`Lexicon`] directly. `Analyzer` exists
//! so the common "look up surface; if absent, try guessing" pattern
//! is one call.

use std::collections::HashSet;
use std::path::Path;

#[cfg(test)]
use crate::analysis::Case;
use crate::analysis::{Analysis, Source, UPOS};
use crate::lexicon::{Lexicon, LoadError};
use crate::paradigm::adjective::{generate_adjective_paradigm, AdjectiveAttested};
use crate::paradigm::noun::{default_plural_guess, generate_noun_paradigm, guess_noun};
use crate::paradigm::verb::{generate_verb_paradigm, VerbAttested};

/// High-level morphological analyzer.
pub struct Analyzer {
    lexicon: Option<Lexicon>,
    fallback_oov: bool,
    swiss_orthography: bool,
}

impl Analyzer {
    /// Construct an empty analyzer (no lexicon loaded). Useful as a
    /// placeholder in tests; production code should call [`open`].
    pub fn empty() -> Self {
        Self {
            lexicon: None,
            fallback_oov: true,
            swiss_orthography: false,
        }
    }

    /// Open the lexicon from disk (FST file + side-table file).
    pub fn open(
        fst_path: impl AsRef<Path>,
        side_path: impl AsRef<Path>,
    ) -> Result<Self, LoadError> {
        Ok(Self {
            lexicon: Some(Lexicon::open(fst_path, side_path)?),
            fallback_oov: true,
            swiss_orthography: false,
        })
    }

    /// Construct directly from an already-loaded [`Lexicon`].
    pub fn from_lexicon(lexicon: Lexicon) -> Self {
        Self {
            lexicon: Some(lexicon),
            fallback_oov: true,
            swiss_orthography: false,
        }
    }

    /// Enable or disable the OOV guesser fallback. Default: on.
    pub fn with_oov_fallback(mut self, enabled: bool) -> Self {
        self.fallback_oov = enabled;
        self
    }

    /// Enable or disable the **Swiss orthography fallback** (`ss → ß`
    /// variants). Default: **off**.
    ///
    /// Turn this on when analysing text that may use Swiss German
    /// conventions (Swiss federal/cantonal publications, NZZ articles,
    /// most Liechtenstein text, web text whose origin is unknown). The
    /// lexicon is built from Wiktionary's Standard German entries (with
    /// `ß`); enabling this flag tries `ss → ß` substitutions on every
    /// surface containing `ss` and unions the resulting analyses with
    /// the direct ones. See [`swiss_orthography_variants`] for the rule.
    ///
    /// Off by default because, for strict Standard German text, the
    /// extra lookups add no value and slightly broaden the analysis
    /// set with low-relevance ß-form candidates (e.g. pre-1996
    /// spellings of `dass` → `daß`).
    pub fn with_swiss_orthography(mut self, enabled: bool) -> Self {
        self.swiss_orthography = enabled;
        self
    }

    /// Return all morphological analyses of `surface`.
    ///
    /// Behaviour:
    /// 1. If a lexicon is loaded, look up `surface` there. If the
    ///    lookup returns any analyses, those are returned (tagged
    ///    `Source::Attested` or `Source::Inflected` as the build
    ///    pipeline recorded them).
    /// 2. **Swiss-orthography fallback** (opt-in via
    ///    [`with_swiss_orthography`]): when the flag is on, probe
    ///    `ss → ß` variants of the surface and union the results.
    ///    Swiss German uses `ss` uniformly where Standard German uses
    ///    `ß` after long vowels/diphthongs: `heisst → heißt`,
    ///    `Strasse → Straße`. Off by default — turn on for Swiss/web
    ///    text where the lexicon (Standard-German Wiktionary) won't
    ///    match Swiss surfaces directly.
    /// 3. Otherwise, if OOV fallback is enabled, the suffix-based
    ///    noun guesser produces best-effort analyses tagged
    ///    `Source::Predicted`.
    ///
    /// Returns an empty vector when neither path produces anything.
    pub fn analyze(&self, surface: &str) -> Vec<Analysis> {
        if let Some(lex) = &self.lexicon {
            let mut hits = lex.analyze(surface);
            // Swiss `ss` → Standard `ß` UNION (gated by the
            // `swiss_orthography` flag — off by default). When enabled,
            // we probe each ss→ß variant and merge resulting analyses
            // with the direct ones. For each Swiss-resolved analysis
            // we emit TWO copies:
            //   1. lemma in its lexicon form (with `ß`), matching UD
            //      treebanks that lemmatize to Standard German;
            //   2. lemma in the Swiss form (with `ß` → `ss`), matching
            //      treebanks that keep the input's orthography.
            // Both styles appear in real-world data: `heisst → heißen`
            // (Standard ß), `Strasse → Strasse` (Swiss ss). Emitting
            // both lemma variants covers either convention.
            if self.swiss_orthography {
                for variant in swiss_orthography_variants(surface) {
                    let variant_hits = lex.analyze(&variant);
                    for h in variant_hits {
                        let push_if_new = |hits: &mut Vec<Analysis>, a: Analysis| {
                            if !hits.iter().any(|existing| {
                                existing.lemma == a.lemma
                                    && existing.pos == a.pos
                                    && existing.features == a.features
                            }) {
                                hits.push(a);
                            }
                        };
                        if h.lemma.contains('ß') {
                            let swiss_lemma = h.lemma.replace('ß', "ss");
                            let h_swiss = Analysis {
                                lemma: swiss_lemma.into(),
                                pos: h.pos,
                                features: h.features,
                                source: h.source,
                            };
                            push_if_new(&mut hits, h);
                            push_if_new(&mut hits, h_swiss);
                        } else {
                            push_if_new(&mut hits, h);
                        }
                    }
                }
            }
            if !hits.is_empty() {
                return hits;
            }
            // Hyphenated-compound fallback. When the surface contains a
            // hyphen and neither direct lookup nor the Swiss path
            // produced anything, try treating it as a hyphenated
            // compound (Palmöl-Importe, Volkswagen-Konzern,
            // Eis-Tee-Latte). German hyphenated nouns are right-headed
            // — the rightmost element carries the inflection and POS,
            // and the lemma is constructed as `left-right.lemma`.
            if surface.contains('-') {
                if let Some(h) = self.analyze_hyphenated(surface, 0) {
                    return h;
                }
            }
            // Solid-compound fallback. Nothing attested and no hyphen:
            // try decomposing the whole surface into a known left part +
            // a known nominal head (Volksinitiative → Volk +s+
            // Initiative). German compounds are right-headed, so the
            // head's gender/number/case drive the analysis — far better
            // than the Strong-Masc default the OOV guesser would emit.
            let solid = lex.analyze_solid_compound(surface);
            if !solid.is_empty() {
                return solid;
            }
        }
        if self.fallback_oov {
            let mut out = Vec::new();
            // Try both noun and verb OOV paths. The German script gives a
            // strong (but not perfect) signal: nouns are capitalised,
            // verb forms are not. We use that as a soft prior — verbs
            // tried first for lowercase tokens, nouns first for
            // capitalised ones, and the other category as a backup if
            // the primary returns nothing.
            let lower_first = surface
                .chars()
                .next()
                .map(|c| c.is_lowercase())
                .unwrap_or(false);
            if lower_first {
                out.extend(self.guess_verb_paradigm_cells(surface));
                if out.is_empty() {
                    out.extend(self.guess_adj_paradigm_cells(surface));
                }
                if out.is_empty() {
                    out.extend(self.guess_noun_paradigm_cells(surface));
                }
            } else {
                out.extend(self.guess_noun_paradigm_cells(surface));
                if out.is_empty() {
                    out.extend(self.guess_adj_paradigm_cells(surface));
                }
                if out.is_empty() {
                    out.extend(self.guess_verb_paradigm_cells(surface));
                }
            }
            out
        } else {
            Vec::new()
        }
    }

    /// Decompose a hyphenated compound like `Palmöl-Importe` into
    /// `(left, right) = ("Palmöl", "Importe")` and synthesise compound
    /// analyses from the right element (which carries the inflection
    /// and POS in German hyphenated nouns).
    ///
    /// Splitting strategy:
    /// 1. Walk hyphen positions from right to left (German is
    ///    right-headed, so the rightmost split is the most likely
    ///    correct one).
    /// 2. For each candidate split, the LEFT must be a NOUN or PROPN
    ///    lemma (citation form), and the RIGHT must have at least one
    ///    NOUN/PROPN analysis. Both constraints rejected by simple
    ///    adjective coordinations like `schwarz-weiß` (`schwarz` is an
    ///    adjective lemma, not a noun) and English borrowings like
    ///    `Stop-and-go` (`Stop` is in our lexicon as a noun but `go`
    ///    isn't a German noun, so the right-validation kicks in).
    /// 3. The first split that produces NOUN/PROPN right analyses
    ///    wins. We synthesise one output analysis per right hit, with
    ///    `lemma = "{left}-{right_lemma}"` and the right's POS and
    ///    features. The synthesised compound is tagged
    ///    `Source::Composed`: the whole compound was never attested as a
    ///    unit (so not `Attested`), but every part is in the lexicon, so it
    ///    ranks above out-of-vocabulary `Predicted` guesses.
    ///
    /// Relationship to the SOLID-compound splitter
    /// [`Lexicon::split_compound_detailed`]: that splitter handles
    /// compounds written without a hyphen (`Bundestag`), so it must
    /// guess both the split points AND any Fugenelement (linker, e.g.
    /// the `-es-` in `Bund+es+Tag`), and it ranks competing splits.
    /// Here the hyphen is an explicit, author-supplied boundary: the
    /// split points are given and hyphenated compounds are typically
    /// linker-free (`Palmöl-Import`, not `Palmöls-Import`). That is why
    /// this path is deliberately simpler — no Fugenelemente, no
    /// scoring — and does not delegate to that splitter. Recursion on
    /// the right is bounded to depth 5 (matching the solid splitter,
    /// see `split_compound_detailed_into`) to cap work on pathological
    /// multi-hyphen input.
    ///
    /// Returns `None` if no split works; the caller then falls through
    /// to the OOV path.
    fn analyze_hyphenated(&self, surface: &str, depth: usize) -> Option<Vec<Analysis>> {
        use crate::analysis::PackedFeatures;
        if depth > 5 {
            return None;
        }
        let lex = self.lexicon.as_ref()?;
        let positions: Vec<usize> = surface.match_indices('-').map(|(i, _)| i).collect();
        // Rightmost-first: German hyphenated nouns are right-headed.
        // For multi-hyphen surfaces like `Eis-Tee-Latte` the rightmost
        // split first asks: is `Eis-Tee` a noun lemma? Usually not. The
        // next split tries `Eis` (yes) + `Tee-Latte` and recurses on
        // the right side via the analyzer's own hyphen path, so the
        // chain resolves bottom-up.
        for &pos in positions.iter().rev() {
            let left = &surface[..pos];
            let right = &surface[pos + 1..];
            if left.is_empty() || right.is_empty() {
                continue;
            }
            // LEFT must be a noun or proper-noun lemma. We accept both
            // because compounds like `Volkswagen-Konzern` lead with a
            // PROPN, and `Palmöl-Importe` leads with a common NOUN.
            let left_is_nominal_lemma =
                lex.is_lemma_of_pos(left, UPOS::NOUN) || lex.is_lemma_of_pos(left, UPOS::PROPN);
            if !left_is_nominal_lemma {
                continue;
            }
            // RIGHT carries the morphology. Resolve via direct lookup
            // first; if empty, recurse into analyze_hyphenated so
            // multi-hyphen chains work. We deliberately do NOT call
            // back into self.analyze() — that would invoke the OOV
            // path, and we don't want to build compound analyses on
            // top of guessed bases (would chain `Predicted` sources and
            // emit low-confidence speculation).
            let right_hits = {
                let direct = lex.analyze(right);
                if direct.is_empty() && right.contains('-') {
                    self.analyze_hyphenated(right, depth + 1)
                        .unwrap_or_default()
                } else {
                    direct
                }
            };
            // Synthesise one compound analysis per DISTINCT right hit.
            // Dedup by (lemma, pos, packed features): every synthesised
            // analysis is forced to Source::Composed, so two right hits
            // that differ only in their source (e.g. one Attested, one
            // Inflected for the same form) would otherwise collapse to
            // byte-identical compounds. Distinct features are preserved.
            let mut synthesised: Vec<Analysis> = Vec::new();
            let mut seen: HashSet<(String, u8, u32)> = HashSet::new();
            for a in right_hits {
                if a.pos != UPOS::NOUN && a.pos != UPOS::PROPN {
                    continue;
                }
                let lemma = format!("{left}-{}", a.lemma);
                let key = (
                    lemma.clone(),
                    a.pos as u8,
                    PackedFeatures::pack(a.features).0,
                );
                if !seen.insert(key) {
                    continue;
                }
                synthesised.push(Analysis {
                    lemma: lemma.into(),
                    pos: a.pos,
                    features: a.features,
                    // Built from parts that are all in the lexicon (left is an
                    // attested nominal lemma, right resolved by lexicon lookup
                    // — never the OOV path), but never attested as a whole
                    // word: tag Composed rather than inheriting the right
                    // part's source (which would mislabel it Attested) or
                    // Predicted (which is for unknown lemmas).
                    source: Source::Composed,
                });
            }
            if !synthesised.is_empty() {
                return Some(synthesised);
            }
        }
        None
    }

    /// OOV path for adjective-shaped surfaces.
    ///
    /// Strategy: peel a small set of German adjective endings off
    /// `surface` and treat the stem as an adjective lemma hypothesis.
    /// Generate the full adjective paradigm and emit any cell that
    /// matches the original surface, with case-insensitive comparison
    /// of the first character (so sentence-initial `"Letzte"`
    /// resolves to lemma `"letzt"` via paradigm cell `"letzte"`).
    ///
    /// Limitations:
    /// - Suppletive comparatives/superlatives (gut/besser/best) are
    ///   not recovered; the lemma must already be in the lexicon.
    /// - Substantivised adjectives (`Der Große ging`) match by
    ///   case-folding the surface against the predicative paradigm
    ///   cells; the analysis still uses `UPOS::ADJ`, not `UPOS::NOUN`.
    fn guess_adj_paradigm_cells(&self, surface: &str) -> Vec<Analysis> {
        let mut out = Vec::new();
        let mut seen: HashSet<(String, u8, u32)> = HashSet::new();

        // Pathway 1: surface IS the lemma (bare predicative form).
        try_adj_lemma_hypothesis(surface, surface, &mut out, &mut seen);
        // Pathway 1b: lowercased surface as lemma (sentence-initial
        // capitalisation, e.g. "Schön" at the start of a sentence
        // refers to the lemma "schön").
        let lower_surface = lowercase_first(surface);
        if lower_surface != surface {
            try_adj_lemma_hypothesis(&lower_surface, surface, &mut out, &mut seen);
        }

        // Pathway 2: suffix-stripped lemma. Order matters: longer
        // first so "schöneren" tries the -en strip before -e.
        for &suffix in ADJ_INFLECTION_SUFFIXES {
            if let Some(stem) = surface.strip_suffix(suffix) {
                if stem.is_empty() || stem == surface {
                    continue;
                }
                try_adj_lemma_hypothesis(stem, surface, &mut out, &mut seen);

                // Capitalisation fallback: if the input started with
                // an uppercase letter (sentence-initial or
                // substantivised), also try the lower-cased lemma.
                let lower = lowercase_first(stem);
                if lower != stem {
                    try_adj_lemma_hypothesis(&lower, surface, &mut out, &mut seen);
                }
            }
        }
        out
    }

    /// OOV path for verb-shaped surfaces.
    ///
    /// Strategy: peel a small set of German verb endings off the
    /// surface to obtain a candidate weak-verb infinitive (stem + en).
    /// For each candidate, synthesise a regular weak-verb paradigm and
    /// return any cell that matches the original surface.
    ///
    /// Limitations:
    /// - Strong verbs whose past stem differs from the present stem
    ///   (e.g. `geben` → `gab`) cannot be recovered from past forms.
    /// - Partizip Perfekt without `ge-` prefix (Latin loans like
    ///   `studiert`) is not handled.
    /// - Separable verbs and reflexives are out of scope.
    fn guess_verb_paradigm_cells(&self, surface: &str) -> Vec<Analysis> {
        let mut out = Vec::new();
        let mut seen: HashSet<(String, u8, u32)> = HashSet::new();

        // Pathway 0: surface IS the infinitive (or 1Pl/3Pl Pres Ind).
        if surface.ends_with("en") && surface.len() > 2 {
            try_verb_lemma_hypothesis(surface, surface, &mut out, &mut seen);
        }

        // Pathway 1: weak-suffix stripping → candidate weak infinitive.
        for &(suffix, infinitive_addition) in VERB_SUFFIX_PATTERNS {
            if let Some(stem) = surface.strip_suffix(suffix) {
                if stem.is_empty() {
                    continue;
                }
                let candidate_inf = format!("{stem}{infinitive_addition}");
                if !candidate_inf.ends_with("en") || candidate_inf.len() <= 2 {
                    continue;
                }
                try_verb_lemma_hypothesis(&candidate_inf, surface, &mut out, &mut seen);
            }
        }

        // Pathway 2: Partizip Perfekt with the `ge-` prefix. Strip both
        // the prefix and the typical `-t` (weak) / `-en` (strong) suffix
        // to reconstruct the candidate infinitive.
        if surface.starts_with("ge") && surface.len() > 3 {
            if let Some(inner) = surface.strip_prefix("ge") {
                if let Some(stem) = inner.strip_suffix('t') {
                    if !stem.is_empty() {
                        let cand = format!("{stem}en");
                        try_verb_lemma_hypothesis(&cand, surface, &mut out, &mut seen);
                    }
                }
                if let Some(stem) = inner.strip_suffix("en") {
                    if !stem.is_empty() {
                        let cand = format!("{stem}en");
                        try_verb_lemma_hypothesis(&cand, surface, &mut out, &mut seen);
                    }
                }
            }
        }

        out
    }

    /// Apply the suffix-based noun guesser to `surface` and return any
    /// paradigm cells whose generated form matches `surface`. The
    /// returned analyses are tagged `Source::Predicted`.
    ///
    /// Two pathways are tried:
    ///   1. **Surface = lemma.** The user looked up an uninflected
    ///      citation form. The Nom-Sg cell of the guessed paradigm
    ///      will match.
    ///   2. **Suffix stripping.** Standard German inflection suffixes
    ///      (-ern / -en / -es / -er / -n / -e / -s) are peeled off
    ///      `surface` one at a time. Each candidate stem is treated as
    ///      a lemma hypothesis, the paradigm is generated, and any
    ///      cell that matches the original `surface` produces an
    ///      analysis. This recovers things like
    ///      `"Quitschen" → lemma "Quitsch", Dat Pl Masc`.
    ///
    /// Known limitations:
    /// - Umlaut plurals (`Männer` from `Mann`) cannot be recovered
    ///   from surface alone — there's no signal in `"Männer"` that
    ///   the lemma was `"Mann"`.
    /// - Weak-masculine -er nouns (`Bauer → Bauern`) fall through to
    ///   Strong Masc and produce the wrong cell. The -er suffix is
    ///   too ambiguous in German (cf. agent nouns Lehrer/Bäcker) to
    ///   default to WeakMasc.
    fn guess_noun_paradigm_cells(&self, surface: &str) -> Vec<Analysis> {
        let mut out = Vec::new();
        let mut seen: HashSet<(String, u8, u32)> = HashSet::new();

        // Pathway 1: surface as lemma.
        try_lemma_hypothesis(surface, surface, &mut out, &mut seen);

        // Pathway 2: suffix-stripped lemma hypotheses.
        for &suffix in NOUN_INFLECTION_SUFFIXES {
            if let Some(stem) = surface.strip_suffix(suffix) {
                if stem.is_empty() || stem == surface {
                    continue;
                }
                try_lemma_hypothesis(stem, surface, &mut out, &mut seen);
            }
        }
        out
    }
}

/// German noun inflection suffixes, longest first so longer matches
/// are tried before shorter sub-suffixes. The empty-suffix case is
/// handled separately in `guess_noun_paradigm_cells` as pathway 1.
const NOUN_INFLECTION_SUFFIXES: &[&str] = &[
    "ern", // Bücher → Büchern (Dat Pl)
    "en",  // Tische → Tischen (Dat Pl), Frau → Frauen, Bauer → Bauern (weak)
    "es",  // Tisch → Tisches (Gen Sg)
    "er",  // Buch → Bücher (umlaut plural — won't actually recover umlaut)
    "n",   // Junge → Jungen
    "e",   // Tisch → Tische (plural)
    "s",   // Auto → Autos, Hund → Hunds
];

/// Try one lemma hypothesis: guess class/gender for `lemma`, build a
/// best-effort default plural so plural-cell matching can succeed,
/// generate the paradigm, and push any cell that matches
/// `original_surface` into `out` (deduplicated by (lemma, pos,
/// packed_features)).
fn try_lemma_hypothesis(
    lemma: &str,
    original_surface: &str,
    out: &mut Vec<Analysis>,
    seen: &mut HashSet<(String, u8, u32)>,
) {
    use crate::analysis::PackedFeatures;
    for guess in guess_noun(lemma) {
        let plural = default_plural_guess(lemma, guess.gender, guess.class);
        let cells = generate_noun_paradigm(lemma, guess.gender, guess.class, plural.as_deref());
        for (form, mut analysis) in cells {
            if form != original_surface {
                continue;
            }
            let key = (
                analysis.lemma.to_string(),
                analysis.pos as u8,
                PackedFeatures::pack(analysis.features).0,
            );
            if seen.insert(key) {
                analysis.source = Source::Predicted;
                out.push(analysis);
            }
        }
    }
}

/// Adjective inflection suffixes (longest first).
const ADJ_INFLECTION_SUFFIXES: &[&str] = &["en", "em", "es", "er", "e"];

/// Try one adjective-lemma hypothesis: generate the paradigm, match
/// the original surface (with case-insensitive first-character
/// comparison so sentence-initial capitalisation doesn't block
/// the match), push deduplicated analyses.
fn try_adj_lemma_hypothesis(
    lemma: &str,
    original_surface: &str,
    out: &mut Vec<Analysis>,
    seen: &mut HashSet<(String, u8, u32)>,
) {
    use crate::analysis::PackedFeatures;
    let inputs = AdjectiveAttested {
        lemma,
        komparativ: None,
        superlativ: None,
    };
    for (form, mut analysis) in generate_adjective_paradigm(&inputs) {
        let match_exact = form == original_surface;
        let match_capfold =
            !match_exact && lowercase_first(&form) == lowercase_first(original_surface);
        if !match_exact && !match_capfold {
            continue;
        }
        let key = (
            analysis.lemma.to_string(),
            analysis.pos as u8,
            PackedFeatures::pack(analysis.features).0,
        );
        if seen.insert(key) {
            analysis.source = Source::Predicted;
            out.push(analysis);
        }
    }
}

/// Lower-case only the first Unicode scalar of `s`. Leaves the rest
/// untouched. Used to fold sentence-initial capitalisation for the
/// adjective-OOV pathway.
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Yield Swiss-orthography variants of `surface` by replacing one or
/// more `ss` substrings with `ß`. The Swiss-German convention uses
/// `ss` uniformly where Standard German uses `ß` (after long vowels
/// and diphthongs); Wiktionary's entries follow the 1996 Standard
/// rules, so to recognise Swiss text we map back.
///
/// Strategy:
/// - 0 occurrences of `ss` → no variants (nothing to try)
/// - 1 occurrence → 1 variant (the `ss → ß` swap)
/// - 2-4 occurrences → all 2^N - 1 non-empty subsets, since multi-`ss`
///   compounds need each `ss` slot considered independently (`Strassenpass`
///   could resolve to `Straßenpaß` or `Straßenpass`, etc.)
/// - 5+ occurrences → only the "replace every `ss`" variant, to keep
///   per-lookup work bounded
///
/// Variant generation walks the byte string. `ss` is ASCII so byte-
/// indexing is safe; `ß` is 2 UTF-8 bytes (0xC3 0x9F) so the swap
/// shifts subsequent positions, which is why we collect positions
/// once and apply masks via fresh allocation.
pub(crate) fn swiss_orthography_variants(surface: &str) -> Vec<String> {
    let positions: Vec<usize> = surface.match_indices("ss").map(|(idx, _)| idx).collect();
    let n = positions.len();
    if n == 0 {
        return Vec::new();
    }
    if n > 4 {
        return vec![replace_all_ss_with_eszett(surface)];
    }
    // Bit-mask enumeration of all 2^n - 1 non-empty subsets.
    let mut out = Vec::with_capacity((1 << n) - 1);
    for mask in 1u32..(1u32 << n) {
        out.push(apply_ss_swap_mask(surface, &positions, mask));
    }
    out
}

fn apply_ss_swap_mask(surface: &str, positions: &[usize], mask: u32) -> String {
    let bytes = surface.as_bytes();
    let mut out = String::with_capacity(surface.len());
    let mut cursor = 0;
    for (i, &pos) in positions.iter().enumerate() {
        // Bytes before this `ss`.
        out.push_str(std::str::from_utf8(&bytes[cursor..pos]).unwrap());
        if (mask >> i) & 1 == 1 {
            out.push('ß');
        } else {
            out.push_str("ss");
        }
        cursor = pos + 2;
    }
    // Tail after the last `ss`.
    out.push_str(std::str::from_utf8(&bytes[cursor..]).unwrap());
    out
}

fn replace_all_ss_with_eszett(surface: &str) -> String {
    surface.replace("ss", "ß")
}

/// German verb-form suffixes paired with the suffix to append to the
/// stripped stem to recover a candidate weak-verb infinitive.
///
/// Examples:
///   "liebte"   → strip "te"   → "lieb"  + "en" = "lieben"  ✓
///   "liebst"   → strip "st"   → "lieb"  + "en" = "lieben"  ✓
///   "liebend"  → strip "end"  → "lieb"  + "en" = "lieben"  ✓
///   "lieben"   → strip "en"   → "lieb"  + ""   = "lieb"    (caller
///                rejects; final candidate must end in "en")
///                Actually we also try the surface itself as the inf
///                via the empty-strip pathway.
///
/// Longest first so longer matches take priority.
const VERB_SUFFIX_PATTERNS: &[(&str, &str)] = &[
    ("test", "en"),
    ("ten", "en"),
    ("tet", "en"),
    ("te", "en"),
    ("end", "en"),
    ("st", "en"),
    ("en", ""),
    ("t", "en"),
    ("e", "en"),
];

/// Synthesise a regular weak-verb paradigm for `candidate_inf` and
/// emit any cell that matches `original_surface`.
fn try_verb_lemma_hypothesis(
    candidate_inf: &str,
    original_surface: &str,
    out: &mut Vec<Analysis>,
    seen: &mut HashSet<(String, u8, u32)>,
) {
    use crate::analysis::PackedFeatures;

    let stem = &candidate_inf[..candidate_inf.len() - 2]; // strip "en"
    if stem.is_empty() {
        return;
    }

    // Regular weak-verb endings. Strong verbs with stem-vowel
    // alternation can't be recovered without lexical info; the FST
    // lookup already covers the attested cases.
    let pres_1sg = format!("{stem}e");
    let pres_2sg = if stem_needs_e_link(stem) {
        format!("{stem}est")
    } else {
        format!("{stem}st")
    };
    let pres_3sg = if stem_needs_e_link(stem) {
        format!("{stem}et")
    } else {
        format!("{stem}t")
    };
    let past_1sg = format!("{stem}te");
    let imp_sg = format!("{stem}e");
    let imp_pl = if stem_needs_e_link(stem) {
        format!("{stem}et")
    } else {
        format!("{stem}t")
    };
    let ptc_perf = format!("ge{stem}t");

    let inputs = VerbAttested {
        infinitive: candidate_inf,
        present_1sg: Some(&pres_1sg),
        present_2sg: Some(&pres_2sg),
        present_3sg: Some(&pres_3sg),
        past_1sg: Some(&past_1sg),
        konj_ii_1sg: Some(&past_1sg),
        imperativ_sg: Some(&imp_sg),
        imperativ_pl: Some(&imp_pl),
        partizip_perf: Some(&ptc_perf),
    };

    for (form, mut analysis) in generate_verb_paradigm(&inputs) {
        if form != original_surface {
            continue;
        }
        let key = (
            analysis.lemma.to_string(),
            analysis.pos as u8,
            PackedFeatures::pack(analysis.features).0,
        );
        if seen.insert(key) {
            analysis.source = Source::Predicted;
            out.push(analysis);
        }
    }
}

#[inline]
fn stem_needs_e_link(stem: &str) -> bool {
    let last = stem.chars().last().unwrap_or(' ');
    matches!(last, 't' | 'd')
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Features, Gender, Number, UPOS};
    use crate::lexicon::LexiconBuilder;

    #[test]
    fn analyzer_uses_lexicon_when_available() {
        let mut b = LexiconBuilder::new();
        b.add(
            "Tisch",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap());

        let hits = analyzer.analyze("Tisch");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, Source::Attested);
    }

    #[test]
    fn analyzer_falls_back_to_oov_guesser() {
        // Empty lexicon — every surface is OOV.
        let analyzer = Analyzer::empty();
        let hits = analyzer.analyze("Zeitung");
        // Suffix -ung → Fem Strong; Nom Sg cell matches the surface.
        assert!(!hits.is_empty(), "expected at least one guessed analysis");
        assert!(hits.iter().any(|a| a.source == Source::Predicted));
        assert!(hits.iter().any(|a| a.features.gender == Some(Gender::Fem)));
    }

    #[test]
    fn analyzer_oov_fallback_can_be_disabled() {
        let analyzer = Analyzer::empty().with_oov_fallback(false);
        let hits = analyzer.analyze("Zeitung");
        assert!(hits.is_empty());
    }

    #[test]
    fn oov_recovers_dative_plural_via_suffix_stripping() {
        // "Quitschen" — a Dat Pl form of made-up masc "Quitsch".
        // The lexicon is empty so this exercises the suffix-stripping
        // path: peel "-en", treat "Quitsch" as the lemma, generate a
        // Strong Masc paradigm whose Dat Pl ("Quitsche" + n = "Quitschen")
        // matches the input.
        let analyzer = Analyzer::empty();
        let hits = analyzer.analyze("Quitschen");
        assert!(!hits.is_empty(), "expected at least one OOV hit");
        let dat_pl = hits
            .iter()
            .find(|a| a.features.case == Some(Case::Dat) && a.features.number == Some(Number::Pl));
        assert!(dat_pl.is_some(), "expected a Dat Pl guess in {hits:#?}");
        let g = dat_pl.unwrap();
        assert_eq!(g.lemma, "Quitsch");
        assert_eq!(g.source, Source::Predicted);
    }

    #[test]
    fn oov_recovers_gen_sg_via_suffix_stripping() {
        // "Quitsches" → lemma "Quitsch", Gen Sg masc.
        let analyzer = Analyzer::empty();
        let hits = analyzer.analyze("Quitsches");
        let gen_sg = hits
            .iter()
            .find(|a| a.features.case == Some(Case::Gen) && a.features.number == Some(Number::Sg));
        assert!(gen_sg.is_some(), "expected Gen Sg in {hits:#?}");
        assert_eq!(gen_sg.unwrap().lemma, "Quitsch");
    }

    #[test]
    fn oov_suffix_strip_uses_strong_signal_when_available() {
        // "Fassungen" — lemma "Fassung" (-ung → Fem Strong). Strip
        // "-en" then guess; the -ung suffix on the stripped lemma
        // gives a High-confidence Fem Strong hypothesis whose Pl
        // cells match.
        let analyzer = Analyzer::empty();
        let hits = analyzer.analyze("Fassungen");
        assert!(
            hits.iter().any(|a| a.lemma == "Fassung"
                && a.features.gender == Some(Gender::Fem)
                && a.features.number == Some(Number::Pl)),
            "missing Fem Pl Fassung analysis in {hits:#?}"
        );
    }

    #[test]
    fn oov_verb_recovers_weak_past() {
        // "liebte" — Past Ind 1Sg or 3Sg, Konj II 1Sg or 3Sg of "lieben".
        let analyzer = Analyzer::empty();
        let hits = analyzer.analyze("liebte");
        use crate::analysis::{Mood, Person, Tense, VerbForm, UPOS};
        let past_1sg = hits.iter().find(|a| {
            a.pos == UPOS::VERB
                && a.features.person == Some(Person::P1)
                && a.features.number == Some(Number::Sg)
                && a.features.tense == Some(Tense::Past)
                && a.features.mood == Some(Mood::Ind)
                && a.features.form == Some(VerbForm::Fin)
        });
        assert!(
            past_1sg.is_some(),
            "expected verb 1Sg Past Ind in {hits:#?}"
        );
        assert_eq!(past_1sg.unwrap().lemma, "lieben");
        assert_eq!(past_1sg.unwrap().source, Source::Predicted);
    }

    #[test]
    fn oov_verb_recovers_2sg_past() {
        // "liebtest" → lemma "lieben", 2Sg Past Ind + 2Sg Konj II.
        let analyzer = Analyzer::empty();
        let hits = analyzer.analyze("liebtest");
        use crate::analysis::{Mood, Person, Tense};
        let two_sg_past = hits.iter().find(|a| {
            a.features.person == Some(Person::P2)
                && a.features.number == Some(Number::Sg)
                && a.features.tense == Some(Tense::Past)
                && a.features.mood == Some(Mood::Ind)
        });
        assert!(two_sg_past.is_some(), "expected 2Sg Past in {hits:#?}");
        assert_eq!(two_sg_past.unwrap().lemma, "lieben");
    }

    #[test]
    fn oov_verb_recovers_infinitive_via_en_strip() {
        // "spielen" (lowercase verb) → infinitive guess.
        let analyzer = Analyzer::empty();
        let hits = analyzer.analyze("spielen");
        use crate::analysis::VerbForm;
        let inf = hits.iter().find(|a| a.features.form == Some(VerbForm::Inf));
        assert!(inf.is_some(), "expected Inf in {hits:#?}");
        assert_eq!(inf.unwrap().lemma, "spielen");
        assert_eq!(inf.unwrap().source, Source::Predicted);
    }

    #[test]
    fn oov_verb_partizip_perfekt_recovered() {
        // "geliebt" → lemma "lieben", PtcPerf via strip "-t" + "en".
        // The "en"-strip pathway also fires but produces "geliebten"
        // candidate which doesn't match.
        let analyzer = Analyzer::empty();
        let hits = analyzer.analyze("geliebt");
        use crate::analysis::VerbForm;
        let ptc = hits
            .iter()
            .find(|a| a.features.form == Some(VerbForm::PtcPerf));
        assert!(ptc.is_some(), "expected PtcPerf in {hits:#?}");
        assert_eq!(ptc.unwrap().lemma, "lieben");
    }

    #[test]
    fn swiss_variants_zero_ss_yields_empty() {
        assert!(swiss_orthography_variants("Berlin").is_empty());
        assert!(swiss_orthography_variants("Hund").is_empty());
    }

    #[test]
    fn swiss_variants_single_ss() {
        let v = swiss_orthography_variants("heisst");
        assert_eq!(v, vec!["heißt"]);
    }

    #[test]
    fn swiss_variants_strasse() {
        // Strasse → Straße (one swap).
        let v = swiss_orthography_variants("Strasse");
        assert_eq!(v, vec!["Straße"]);
    }

    #[test]
    fn swiss_variants_two_ss_yields_all_subsets() {
        // 2 occurrences → 3 variants: swap 1st only, swap 2nd only, swap both.
        let v = swiss_orthography_variants("Strassenpass");
        assert_eq!(v.len(), 3);
        assert!(v.contains(&"Straßenpass".to_string()));
        assert!(v.contains(&"Strassenpaß".to_string()));
        assert!(v.contains(&"Straßenpaß".to_string()));
    }

    #[test]
    fn swiss_variants_high_count_falls_back_to_all_at_once() {
        // 5+ occurrences cap to the single all-replaced variant.
        let surface = "ssssssssss"; // 5 `ss` (overlapping wouldn't normally happen)
        let v = swiss_orthography_variants(surface);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn analyzer_splits_hyphenated_compound() {
        // Palmöl + Importe → lemma "Palmöl-Import", inflection from Importe.
        let mut b = LexiconBuilder::new();
        b.add(
            "Palmöl",
            "Palmöl",
            UPOS::NOUN,
            Features::noun_form(Gender::Neut, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Importe",
            "Import",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Pl, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap())
            .with_oov_fallback(false);
        let hits = analyzer.analyze("Palmöl-Importe");
        assert!(!hits.is_empty(), "hyphen-split failed for Palmöl-Importe");
        let h = hits.iter().find(|a| a.pos == UPOS::NOUN).unwrap();
        assert_eq!(h.lemma, "Palmöl-Import");
        assert_eq!(h.features.number, Some(Number::Pl));
        // Synthesised compound from parts that are all in the lexicon, not
        // attested as a whole → Composed (not the right part's Attested tag).
        assert_eq!(h.source, Source::Composed);
    }

    #[test]
    fn analyzer_splits_multi_hyphen_compound_recursively() {
        // Eis + Tee + Latte. The first split (Eis-Tee | Latte) fails
        // because "Eis-Tee" isn't a lemma; the second (Eis | Tee-Latte)
        // recurses on the right and constructs the chain.
        let mut b = LexiconBuilder::new();
        for (sur, lem) in [("Eis", "Eis"), ("Tee", "Tee"), ("Latte", "Latte")] {
            b.add(
                sur,
                lem,
                UPOS::NOUN,
                Features::noun_form(Gender::Fem, Number::Sg, Case::Nom),
                Source::Attested,
            )
            .unwrap();
        }
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap())
            .with_oov_fallback(false);
        let hits = analyzer.analyze("Eis-Tee-Latte");
        assert!(!hits.is_empty(), "multi-hyphen split failed");
        let h = hits.iter().find(|a| a.pos == UPOS::NOUN).unwrap();
        assert_eq!(h.lemma, "Eis-Tee-Latte");
    }

    /// Build a lexicon with `Volk` (+ Gen-Sg `Volks` for the Fugen-s)
    /// and `Initiative` (Sg + Pl), used by the solid-compound tests.
    fn solid_compound_lexicon() -> Lexicon {
        let mut b = LexiconBuilder::new();
        b.add(
            "Volk",
            "Volk",
            UPOS::NOUN,
            Features::noun_form(Gender::Neut, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        // Gen-Sg `Volks` validates the Fugen-s linker in Volk+s+….
        b.add(
            "Volks",
            "Volk",
            UPOS::NOUN,
            Features::noun_form(Gender::Neut, Number::Sg, Case::Gen),
            Source::Attested,
        )
        .unwrap();
        for case in [Case::Nom, Case::Gen, Case::Dat, Case::Acc] {
            b.add(
                "Initiative",
                "Initiative",
                UPOS::NOUN,
                Features::noun_form(Gender::Fem, Number::Sg, case),
                Source::Attested,
            )
            .unwrap();
        }
        b.add(
            "Initiativen",
            "Initiative",
            UPOS::NOUN,
            Features::noun_form(Gender::Fem, Number::Pl, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        Lexicon::from_bytes(fst, side).unwrap()
    }

    #[test]
    fn analyzer_splits_solid_compound_singular() {
        // `Volksinitiative` is not attested as a whole, but decomposes
        // into `Volk` +s+ `Initiative`. German compounds are
        // right-headed, so the head's gender (Fem) and case drive the
        // analysis — not the Strong-Masc OOV fallback.
        let analyzer = Analyzer::from_lexicon(solid_compound_lexicon()).with_oov_fallback(false);
        let hits = analyzer.analyze("Volksinitiative");
        assert!(!hits.is_empty(), "solid-compound split failed");
        assert!(
            hits.iter().all(|a| a.features.gender == Some(Gender::Fem)),
            "expected all-Fem head morphology, got {hits:#?}"
        );
        let nom = hits
            .iter()
            .find(|a| a.features.case == Some(Case::Nom) && a.features.number == Some(Number::Sg))
            .expect("missing Fem Sg Nom in solid-compound analysis");
        assert_eq!(nom.lemma, "Volksinitiative");
        assert_eq!(nom.source, Source::Composed);
    }

    #[test]
    fn analyzer_splits_solid_compound_adjectival_head() {
        // `Datenschutzbeauftragter` = `Datenschutz` + substantivised
        // adjective head `Beauftragter` (the participle `beauftragt`
        // declined adjectivally, masc-nom-sg-strong). The head isn't a
        // noun lemma, so it resolves via the adjective paradigm; the
        // compound lemma is the masc-nom-sg-strong citation.
        let mut b = LexiconBuilder::new();
        b.add(
            "Datenschutz",
            "Datenschutz",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        // The adjectival head is only nominalised when its base is an
        // attested adjective/participle — here the ADJ `beauftragt`.
        b.add(
            "beauftragt",
            "beauftragt",
            UPOS::ADJ,
            Features::empty(),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap())
            .with_oov_fallback(false);
        let hits = analyzer.analyze("Datenschutzbeauftragter");
        assert!(!hits.is_empty(), "adjectival-head split failed");
        let masc_nom = hits
            .iter()
            .find(|a| {
                a.pos == UPOS::NOUN
                    && a.features.gender == Some(Gender::Masc)
                    && a.features.number == Some(Number::Sg)
                    && a.features.case == Some(Case::Nom)
            })
            .expect("missing Masc Sg Nom in adjectival-head compound");
        assert_eq!(masc_nom.lemma, "Datenschutzbeauftragter");
        assert_eq!(masc_nom.source, Source::Composed);
    }

    #[test]
    fn solid_compound_adjectival_head_requires_attested_base() {
        // Guard against false positives: a surface like `Datenschutzxyte`
        // would otherwise force-split into `Datenschutz` + an OOV-
        // generated adjective head `xyt`, inventing a noun. With no
        // attested adjective/participle base for the head, the
        // adjectival path must NOT fire.
        let mut b = LexiconBuilder::new();
        b.add(
            "Datenschutz",
            "Datenschutz",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap())
            .with_oov_fallback(false);
        let hits = analyzer.analyze("Datenschutzxyte");
        assert!(
            hits.is_empty(),
            "expected no spurious adjectival-head split, got {hits:#?}"
        );
    }

    #[test]
    fn analyzer_splits_solid_compound_inflected_head() {
        // `Volksinitiativen` — the head is the *inflected* plural
        // `Initiativen`, so the compound resolves to Fem Pl with the
        // singular compound lemma.
        let analyzer = Analyzer::from_lexicon(solid_compound_lexicon()).with_oov_fallback(false);
        let hits = analyzer.analyze("Volksinitiativen");
        let pl = hits
            .iter()
            .find(|a| a.features.number == Some(Number::Pl))
            .expect("missing Fem Pl in inflected-head solid compound");
        assert_eq!(pl.features.gender, Some(Gender::Fem));
        assert_eq!(pl.lemma, "Volksinitiative");
        assert_eq!(pl.source, Source::Composed);
    }

    #[test]
    fn analyzer_rejects_hyphenated_when_left_not_nominal() {
        // schwarz-weiß is an adjective coordination, not a noun
        // compound. With only an Adj entry for "schwarz", the
        // hyphen-split path must NOT fire.
        let mut b = LexiconBuilder::new();
        b.add(
            "schwarz",
            "schwarz",
            UPOS::ADJ,
            Features::empty(),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "weiß",
            "weiß",
            UPOS::ADJ,
            Features::empty(),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap())
            .with_oov_fallback(false);
        let hits = analyzer.analyze("schwarz-weiß");
        // Should be empty: schwarz is ADJ, not NOUN, so hyphen-path declines.
        assert!(
            hits.is_empty(),
            "expected no hyphen-split for adj-adj, got {hits:?}"
        );
    }

    /// Build an analyzer over `entries` (all tagged `Source::Attested`)
    /// with the OOV fallback disabled, so tests isolate the hyphen path.
    fn lexicon_analyzer(entries: &[(&str, &str, UPOS, Features)]) -> Analyzer {
        let mut b = LexiconBuilder::new();
        for &(surface, lemma, pos, features) in entries {
            b.add(surface, lemma, pos, features, Source::Attested)
                .unwrap();
        }
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap()).with_oov_fallback(false)
    }

    #[test]
    fn analyzer_hyphen_accepts_propn_left() {
        // Volkswagen-Konzern: PROPN left + NOUN right. The hyphen path
        // accepts a proper-noun left element (org-style compounds).
        let analyzer = lexicon_analyzer(&[
            ("Volkswagen", "Volkswagen", UPOS::PROPN, Features::empty()),
            (
                "Konzern",
                "Konzern",
                UPOS::NOUN,
                Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            ),
        ]);
        let hits = analyzer.analyze("Volkswagen-Konzern");
        let h = hits
            .iter()
            .find(|a| a.lemma == "Volkswagen-Konzern")
            .unwrap_or_else(|| panic!("expected PROPN-left compound, got {hits:?}"));
        assert_eq!(h.pos, UPOS::NOUN);
        assert_eq!(h.source, Source::Composed);
    }

    #[test]
    fn analyzer_hyphen_rejects_empty_segments() {
        // Degenerate hyphenation must not panic or mis-split. Leading,
        // trailing, and doubled hyphens each leave an empty segment that
        // the split loop skips, so none produce a compound. (OOV off.)
        let analyzer = lexicon_analyzer(&[
            ("Volkswagen", "Volkswagen", UPOS::PROPN, Features::empty()),
            (
                "Konzern",
                "Konzern",
                UPOS::NOUN,
                Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            ),
        ]);
        for surface in ["-Konzern", "Volkswagen-", "Volkswagen--Konzern"] {
            assert!(
                analyzer.analyze(surface).is_empty(),
                "expected no compound for degenerate {surface:?}"
            );
        }
    }

    #[test]
    fn analyzer_hyphen_dedupes_identical_right_analyses() {
        // The right form "Teile" is attested twice with the SAME
        // (lemma, pos, features) but different Source (Attested vs
        // Inflected). Because the compound forces Source::Composed,
        // both would collapse to identical analyses — dedup keeps one.
        let mut b = LexiconBuilder::new();
        b.add(
            "Auto",
            "Auto",
            UPOS::NOUN,
            Features::noun_form(Gender::Neut, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let pl = Features::noun_form(Gender::Masc, Number::Pl, Case::Nom);
        b.add("Teile", "Teil", UPOS::NOUN, pl, Source::Attested)
            .unwrap();
        b.add("Teile", "Teil", UPOS::NOUN, pl, Source::Inflected)
            .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap())
            .with_oov_fallback(false);
        // Premise: the right form really does carry two distinct records.
        assert_eq!(analyzer.analyze("Teile").len(), 2);
        let hits = analyzer.analyze("Auto-Teile");
        let n = hits.iter().filter(|a| a.lemma == "Auto-Teil").count();
        assert_eq!(n, 1, "expected one deduped compound, got {hits:?}");
    }

    #[test]
    fn analyzer_hyphen_preserves_distinct_right_analyses() {
        // When the right form has genuinely distinct analyses (Nom vs
        // Acc), each yields its own compound — dedup must NOT merge them.
        let analyzer = lexicon_analyzer(&[
            (
                "Auto",
                "Auto",
                UPOS::NOUN,
                Features::noun_form(Gender::Neut, Number::Sg, Case::Nom),
            ),
            (
                "Bahn",
                "Bahn",
                UPOS::NOUN,
                Features::noun_form(Gender::Fem, Number::Sg, Case::Nom),
            ),
            (
                "Bahn",
                "Bahn",
                UPOS::NOUN,
                Features::noun_form(Gender::Fem, Number::Sg, Case::Acc),
            ),
        ]);
        let hits = analyzer.analyze("Auto-Bahn");
        let compounds: Vec<_> = hits.iter().filter(|a| a.lemma == "Auto-Bahn").collect();
        assert_eq!(
            compounds.len(),
            2,
            "expected Nom+Acc compounds, got {hits:?}"
        );
        assert!(compounds.iter().any(|a| a.features.case == Some(Case::Nom)));
        assert!(compounds.iter().any(|a| a.features.case == Some(Case::Acc)));
    }

    #[test]
    fn analyzer_hyphen_no_lexicon_yields_nothing() {
        // No lexicon loaded: the lexicon block is skipped entirely, so a
        // hyphenated surface yields nothing without panicking (OOV off).
        let analyzer = Analyzer::empty().with_oov_fallback(false);
        assert!(analyzer.analyze("Palmöl-Importe").is_empty());
    }

    #[test]
    fn analyzer_uses_swiss_fallback_when_flag_enabled() {
        // Build a lexicon containing only the Standard-German form
        // "heißt"; querying the Swiss-spelled "heisst" with the flag
        // ON should round-trip via the orthography fallback.
        let mut b = LexiconBuilder::new();
        b.add(
            "heißt",
            "heißen",
            UPOS::VERB,
            Features::empty(),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap())
            .with_oov_fallback(false)
            .with_swiss_orthography(true);
        let hits = analyzer.analyze("heisst");
        assert!(!hits.is_empty(), "Swiss-orthography fallback failed");
        assert!(hits
            .iter()
            .any(|h| h.lemma == "heißen" && h.pos == UPOS::VERB));
    }

    #[test]
    fn analyzer_swiss_fallback_off_by_default() {
        // Same lexicon as above; without the flag, the Swiss "heisst"
        // surface does NOT resolve.
        let mut b = LexiconBuilder::new();
        b.add(
            "heißt",
            "heißen",
            UPOS::VERB,
            Features::empty(),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap())
            .with_oov_fallback(false);
        let hits = analyzer.analyze("heisst");
        assert!(
            hits.is_empty(),
            "Expected no analyses without the Swiss flag, got {hits:?}"
        );
    }

    #[test]
    fn analyzer_direct_lookup_unaffected_by_swiss_flag() {
        // If a surface is found directly, the Swiss path only adds —
        // it never removes the direct hit. Asserted with the flag on.
        let mut b = LexiconBuilder::new();
        b.add(
            "Kasse",
            "Kasse",
            UPOS::NOUN,
            Features::noun_form(Gender::Fem, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let analyzer = Analyzer::from_lexicon(Lexicon::from_bytes(fst, side).unwrap())
            .with_oov_fallback(false)
            .with_swiss_orthography(true);
        let hits = analyzer.analyze("Kasse");
        // "Kaße" isn't in the lexicon so the Swiss path adds nothing,
        // and the single direct hit comes through unmodified.
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].lemma, "Kasse");
    }

    #[test]
    fn oov_dedupes_same_analysis_from_multiple_strippings() {
        // The Nom Sg "Tisch" is matched by both pathway 1 (surface as
        // lemma) and pathway 2 (no suffix strip would actually produce
        // "Tisch" since none of the noun suffixes apply). Make sure
        // duplicates don't accumulate even when both pathways fire.
        let analyzer = Analyzer::empty();
        let hits = analyzer.analyze("Tisch");
        // Same (lemma, pos, features) should appear at most once.
        let mut keys: Vec<_> = hits
            .iter()
            .map(|a| (a.lemma.clone(), a.pos, a.features))
            .collect();
        keys.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        let original_len = keys.len();
        keys.dedup();
        assert_eq!(
            keys.len(),
            original_len,
            "found duplicate analyses in OOV output"
        );
    }
}
