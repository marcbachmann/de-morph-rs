//! Load a built lexicon from bytes / files and perform analyses.
//!
//! The runtime holds:
//! - an `fst::Map` of the surface bytes,
//! - the side-table bytes (containing the lemma intern table + the
//!   analyses array).
//!
//! Lookups are O(|surface|) on the FST plus a small constant per
//! analysis returned. No allocation per lookup beyond the result `Vec`.

use std::path::Path;

use fst::Map as FstMap;

use crate::analysis::{Analysis, Aux, Case, Gender, Number, PackedFeatures, Source, UPOS};
use crate::lexicon::format::{
    unpack_fst_value, AnalysisRecord, Shape, ANALYSIS_RECORD_SIZE, HEADER_SIZE, MAGIC,
    SHAPE_ENTRY_SIZE, VERSION_MAJOR,
};

/// Errors raised when loading a lexicon.
#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Fst(fst::Error),
    /// Side-table magic bytes do not match.
    BadMagic,
    /// Side-table major version differs from what this build supports.
    UnsupportedVersion {
        found: u16,
        expected: u16,
    },
    /// Side-table header field claims a region that extends past the
    /// end of the file.
    Truncated {
        field: &'static str,
    },
    /// A pos byte was outside the known POS enum range.
    InvalidPos(u8),
    /// A source byte was outside the known Source enum range.
    InvalidSource(u8),
    /// A record referenced a shape id beyond the shape table.
    InvalidShape(u16),
    /// A lemma in the intern table was not valid UTF-8.
    InvalidLemmaUtf8,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Fst(e) => write!(f, "fst error: {e}"),
            Self::BadMagic => write!(f, "bad magic bytes in side table"),
            Self::UnsupportedVersion { found, expected } => write!(
                f,
                "unsupported side-table version: found {found}, expected {expected}"
            ),
            Self::Truncated { field } => write!(f, "truncated side table at {field}"),
            Self::InvalidPos(p) => write!(f, "invalid pos byte: {p}"),
            Self::InvalidSource(s) => write!(f, "invalid source byte: {s}"),
            Self::InvalidShape(s) => write!(f, "shape id out of range: {s}"),
            Self::InvalidLemmaUtf8 => write!(f, "lemma intern table contains invalid UTF-8"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<std::io::Error> for LoadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<fst::Error> for LoadError {
    fn from(e: fst::Error) -> Self {
        Self::Fst(e)
    }
}

/// A compound decomposition with both the part inventory and the
/// linkers (Fugenelemente) between them. Length invariant:
/// `linkers.len() == parts.len() - 1`. The decomposition is
/// well-formed iff `display_concat()` equals the original surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundSplit {
    pub parts: Vec<String>,
    pub linkers: Vec<String>,
}

impl CompoundSplit {
    /// Reassemble the surface form by concatenating `parts[0] +
    /// linkers[0] + parts[1] + linkers[1] + ... + parts[N-1]`, with
    /// the German orthographic convention applied: compound-internal
    /// parts (everything after the first) have their first letter
    /// case-folded to lowercase. This produces the actual surface
    /// shape — e.g. `Bund + es + tag = Bundestag` rather than
    /// `BundesTag`.
    ///
    /// Used for the reassembly invariant test: a well-formed split's
    /// reassemble() must equal the original surface.
    pub fn reassemble(&self) -> String {
        let mut out = String::new();
        for (i, p) in self.parts.iter().enumerate() {
            if i > 0 {
                out.push_str(&self.linkers[i - 1]);
                // German: compound-internal nouns lose their initial
                // capitalisation. Only the FIRST element of a compound
                // keeps its citation-form case.
                let mut chars = p.chars();
                if let Some(first) = chars.next() {
                    for c in first.to_lowercase() {
                        out.push(c);
                    }
                    out.push_str(chars.as_str());
                }
            } else {
                out.push_str(p);
            }
        }
        out
    }

    /// Human-readable rendering: `"Bund + es + Tag"`, with empty
    /// linkers omitted (so `Haus + Arbeit` reads correctly without
    /// inventing a `+ +` between them).
    pub fn display(&self) -> String {
        let mut s = String::new();
        for (i, p) in self.parts.iter().enumerate() {
            if i > 0 {
                let linker = &self.linkers[i - 1];
                if linker.is_empty() {
                    s.push_str(" + ");
                } else {
                    s.push_str(" + ");
                    s.push_str(linker);
                    s.push_str(" + ");
                }
            }
            s.push_str(p);
        }
        s
    }
}

/// In-memory, read-only morphological lexicon.
pub struct Lexicon {
    fst: FstMap<Vec<u8>>,
    side: Vec<u8>,
    /// Offsets parsed once from the header for fast access.
    lemma_offsets_offset: usize,
    lemma_bytes_offset: usize,
    /// Where the analyses section ends; bounds-checked in `analyze`.
    analyses_end: usize,
    num_lemmas: usize,
    #[allow(dead_code)]
    num_analyses: usize,
    /// Interned analysis shapes, decoded once at load and indexed by the
    /// `shape_id` in each record.
    shapes: Vec<Shape>,
}

impl Lexicon {
    /// Open a lexicon by reading the two files at the given paths.
    pub fn open(
        fst_path: impl AsRef<Path>,
        side_path: impl AsRef<Path>,
    ) -> Result<Self, LoadError> {
        let fst_bytes = std::fs::read(fst_path.as_ref())?;
        let side_bytes = std::fs::read(side_path.as_ref())?;
        Self::from_bytes(fst_bytes, side_bytes)
    }

    /// Construct from raw bytes (e.g. for embedded lexica or tests).
    pub fn from_bytes(fst_bytes: Vec<u8>, side_bytes: Vec<u8>) -> Result<Self, LoadError> {
        let fst = FstMap::new(fst_bytes)?;
        if side_bytes.len() < HEADER_SIZE {
            return Err(LoadError::Truncated { field: "header" });
        }
        if side_bytes[0..12] != MAGIC {
            return Err(LoadError::BadMagic);
        }
        let version_major = u16::from_le_bytes(side_bytes[12..14].try_into().unwrap());
        if version_major != VERSION_MAJOR {
            return Err(LoadError::UnsupportedVersion {
                found: version_major,
                expected: VERSION_MAJOR,
            });
        }
        let num_lemmas = u32::from_le_bytes(side_bytes[20..24].try_into().unwrap()) as usize;
        let num_analyses = u32::from_le_bytes(side_bytes[24..28].try_into().unwrap()) as usize;
        let lemma_offsets_offset =
            u32::from_le_bytes(side_bytes[28..32].try_into().unwrap()) as usize;
        let lemma_bytes_offset =
            u32::from_le_bytes(side_bytes[32..36].try_into().unwrap()) as usize;
        let analyses_offset = u32::from_le_bytes(side_bytes[36..40].try_into().unwrap()) as usize;
        let analyses_end = u32::from_le_bytes(side_bytes[40..44].try_into().unwrap()) as usize;
        let num_shapes = u32::from_le_bytes(side_bytes[44..48].try_into().unwrap()) as usize;
        let shape_table_offset =
            u32::from_le_bytes(side_bytes[48..52].try_into().unwrap()) as usize;

        if side_bytes.len() < analyses_end {
            return Err(LoadError::Truncated {
                field: "analyses end",
            });
        }
        if lemma_offsets_offset + (num_lemmas + 1) * 4 > lemma_bytes_offset {
            return Err(LoadError::Truncated {
                field: "lemma offsets",
            });
        }
        let shape_table_end = shape_table_offset + num_shapes * SHAPE_ENTRY_SIZE;
        if shape_table_end > analyses_offset || analyses_offset > side_bytes.len() {
            return Err(LoadError::Truncated {
                field: "shape table",
            });
        }

        // Decode the shape table once (a few hundred entries).
        let mut shapes = Vec::with_capacity(num_shapes);
        for i in 0..num_shapes {
            let start = shape_table_offset + i * SHAPE_ENTRY_SIZE;
            shapes.push(Shape::from_bytes(
                &side_bytes[start..start + SHAPE_ENTRY_SIZE],
            ));
        }

        Ok(Lexicon {
            fst,
            side: side_bytes,
            lemma_offsets_offset,
            lemma_bytes_offset,
            analyses_end,
            num_lemmas,
            num_analyses,
            shapes,
        })
    }

    /// Number of distinct surface forms in the FST.
    pub fn num_surfaces(&self) -> u64 {
        self.fst.len() as u64
    }

    /// Number of unique lemmas.
    pub fn num_lemmas(&self) -> usize {
        self.num_lemmas
    }

    /// Return `true` if `surface` has at least one analysis in the FST.
    pub fn contains(&self, surface: &str) -> bool {
        self.fst.get(surface.as_bytes()).is_some()
    }

    /// Return `true` if `surface` is a citation-form lemma (i.e. one of
    /// its analyses has `lemma == surface`) of the given POS.
    pub fn is_lemma_of_pos(&self, surface: &str, pos: UPOS) -> bool {
        self.analyze(surface)
            .iter()
            .any(|a| a.lemma == surface && a.pos == pos)
    }

    /// Validity rules for a German compound's LEFT part.
    ///
    /// A surface is acceptable as the left side of a compound if it is
    /// any of:
    /// - a noun, adjective, or proper-noun LEMMA (citation form), OR
    /// - an inflected noun or proper-noun form whose grammatical case
    ///   is Genitive OR whose number is Plural, OR
    /// - a verb stem (i.e. `left + "en"` or `left + "n"` is a verb
    ///   lemma — `klär+en` → klären → Klärschlamm).
    ///
    /// This rejects Dat/Acc Sg forms like `Bunde` (Dat Sg of Bund) which
    /// don't form compounds in standard German, while accepting cases
    /// like `Wörter` (Nom Pl of Wort → Wörterbuch), `Bundes` (Gen Sg of
    /// Bund → Bundesregierung), and `Zürich` / `Zürichs` (Propn + Gen
    /// of Zürich → Zürichsee).
    fn is_valid_compound_left(&self, left: &str) -> bool {
        let analyses = self.analyze(left);
        for a in &analyses {
            // Direct lemma of Noun, Adj, or Propn (proper noun).
            if a.lemma == left
                && (a.pos == UPOS::NOUN || a.pos == UPOS::ADJ || a.pos == UPOS::PROPN)
            {
                return true;
            }
            // Noun / Propn inflected form, but only Gen-* or *-Pl allowed.
            if (a.pos == UPOS::NOUN || a.pos == UPOS::PROPN)
                && (a.features.case == Some(Case::Gen) || a.features.number == Some(Number::Pl))
            {
                return true;
            }
        }
        // Verb-stem check: accept `klär` as a compound left iff
        // `klären` is a verb lemma in the lexicon. Verb stems serve as
        // productive compound left parts (waschen → Waschmaschine,
        // klären → Klärschlamm). Caller passes lowercased forms when
        // looking for this case (verb lemmas are lowercase in German).
        if self.is_lemma_of_pos(&format!("{left}en"), UPOS::VERB) {
            return true;
        }
        if self.is_lemma_of_pos(&format!("{left}n"), UPOS::VERB) {
            return true;
        }
        false
    }

    /// Validity rules for a Fugenelement (linker) given the left part.
    ///
    /// The linker is valid if EITHER:
    /// 1. `left + linker` is itself an attested inflected form of
    ///    `left`'s lemma — this covers the masculine/neuter
    ///    Gen-Sg case (Bund + es → Bundes) and noun plurals (Sonne + n →
    ///    Sonnen), AND/OR
    /// 2. the linker is a productive Fugen-s or Fugen-es AND `left` is a
    ///    feminine noun lemma whose declension lacks a genitive -s.
    ///    German feminine nouns don't take -s in genitive, yet still
    ///    take Fugen-s in compounds (Geburt + s → Geburtsort,
    ///    Liebe + s → Liebesbrief). To avoid the Hau+s+Arbeit trap, we
    ///    additionally require that `left + linker` is NOT itself a
    ///    distinct attested word.
    ///
    /// Empty linker is always valid (the most common German compound
    /// shape: Haus + Tür → Haustür).
    fn is_valid_compound_linker(&self, left: &str, linker: &str) -> bool {
        if linker.is_empty() {
            return true;
        }
        let combined = format!("{left}{linker}");
        let combined_analyses = self.analyze(&combined);
        // Rule 1: combined is an attested inflected form of left's lemma.
        if combined_analyses
            .iter()
            .any(|a| a.lemma == left && (a.pos == UPOS::NOUN || a.pos == UPOS::PROPN))
        {
            return true;
        }
        // Rule 2: Fugen-s/-es for feminine nouns. Only fires when
        // `combined` does NOT itself analyse to anything (preventing
        // the Hau+s spurious split, since "Haus" is its own lemma).
        if matches!(linker, "s" | "es") && combined_analyses.is_empty() {
            for a in &self.analyze(left) {
                if a.lemma == left && a.pos == UPOS::NOUN && a.features.gender == Some(Gender::Fem)
                {
                    return true;
                }
            }
        }
        false
    }

    /// Same as [`split_compound`] but each result preserves the
    /// linker (Fugenelement) used between consecutive parts.
    ///
    /// Returns a `Vec<CompoundSplit>` where each entry exposes:
    /// - `parts`: the constituent lemmas / forms (length N)
    /// - `linkers`: the Fugenelemente between them (length N-1)
    ///
    /// For the canonical Bundestag decomposition:
    ///   `parts = ["Bund", "Tag"]`, `linkers = ["es"]`
    /// which `display()` renders as `"Bund + es + Tag"`. The bare
    /// parts-only output `["Bund", "Tag"]` reads ambiguously as
    /// `Bundtag` (which is not a word) so the detailed form is
    /// recommended whenever the consumer wants to display or
    /// reason about the morphology.
    pub fn split_compound_detailed(&self, surface: &str) -> Vec<CompoundSplit> {
        let mut out = Vec::new();
        self.split_compound_detailed_into(surface, &mut Vec::new(), &mut Vec::new(), &mut out, 0);
        out
    }

    fn split_compound_detailed_into(
        &self,
        remainder: &str,
        parts: &mut Vec<String>,
        linkers: &mut Vec<String>,
        out: &mut Vec<CompoundSplit>,
        depth: usize,
    ) {
        if depth > 5 {
            return;
        }
        let char_boundaries: Vec<usize> = remainder
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(remainder.len()))
            .collect();
        let total_chars = char_boundaries.len() - 1;
        if total_chars < 6 {
            return;
        }
        for split_at_char in 3..=(total_chars - 3) {
            let split_byte = char_boundaries[split_at_char];
            let left = &remainder[..split_byte];
            // Validate by trying verbatim, capitalised, AND lowercased
            // case. Capitalised handles compound-internal nouns (which
            // appear lowercase in the surface but are nouns in their
            // citation form); lowercased handles adjective-headed
            // compounds (Steil+Küste — surface uppercase, lemma "steil"
            // lowercase). Storage always keeps the surface case so the
            // reassembly invariant holds without first-part case-folding.
            let left_cap = capitalize(left);
            let left_lower = lowercase_first(left);
            let left_valid = self.is_valid_compound_left(left)
                || self.is_valid_compound_left(&left_cap)
                || self.is_valid_compound_left(&left_lower);
            if !left_valid {
                continue;
            }
            // Pick the validated form for the linker check (it expects
            // the lemma-case form: `Bund` not `bund`, `steil` not `Steil`).
            let left_for_linker = if self.is_valid_compound_left(left) {
                left.to_string()
            } else if self.is_valid_compound_left(&left_cap) {
                left_cap.clone()
            } else {
                left_lower.clone()
            };
            let left_form = left.to_string();

            // "er" added 2026-06: 298 attestations in Wiktionary's
            // Fugenelement annotations (5th most common non-empty
            // linker, e.g. Wort+er+Buch = Wörterbuch — the umlaut
            // applies at the same time, which our is_valid_compound_linker
            // check handles by requiring `left+er` to be an attested
            // form of left's lemma).
            for linker in &["", "s", "es", "n", "en", "er"] {
                // Use starts_with on the byte slice — this is correct
                // for both empty linkers and ASCII linkers without
                // risking landing in the middle of a multibyte char
                // (e.g. ü at split_byte+1).
                let tail = &remainder[split_byte..];
                if !tail.starts_with(*linker) {
                    continue;
                }
                let l_bytes = linker.len();
                let right_start_byte = split_byte + l_bytes;
                if right_start_byte >= remainder.len() {
                    continue;
                }
                let right = &remainder[right_start_byte..];

                if !self.is_valid_compound_linker(&left_for_linker, linker) {
                    continue;
                }

                parts.push(left_form.clone());
                linkers.push((*linker).to_string());

                let right_cap = capitalize(right);
                // The rightmost chunk must be a noun OR proper-noun
                // LEMMA — compounds like `Brandenburger Tor` are nouns
                // headed by a proper-noun first element + noun head;
                // place-name endings (`-See`, `-Berg`, `-Stadt`) attach
                // common-noun heads; org-style compounds like
                // `Volkswagen-Konzern` can also head with a proper noun.
                if self.is_lemma_of_pos(&right_cap, UPOS::NOUN)
                    || self.is_lemma_of_pos(&right_cap, UPOS::PROPN)
                {
                    let mut p = parts.clone();
                    let l = linkers.clone();
                    p.push(right_cap.clone());
                    out.push(CompoundSplit {
                        parts: p,
                        linkers: l,
                    });
                } else if self.is_lemma_of_pos(right, UPOS::NOUN)
                    || self.is_lemma_of_pos(right, UPOS::PROPN)
                {
                    let mut p = parts.clone();
                    let l = linkers.clone();
                    p.push(right.to_string());
                    out.push(CompoundSplit {
                        parts: p,
                        linkers: l,
                    });
                }
                self.split_compound_detailed_into(right, parts, linkers, out, depth + 1);
                parts.pop();
                linkers.pop();
            }
        }
    }

    /// Like [`split_compound_detailed`] but sorted by score (best first).
    pub fn split_compound_detailed_ranked(&self, surface: &str) -> Vec<(CompoundSplit, f64)> {
        let splits = self.split_compound_detailed(surface);
        let mut scored: Vec<(CompoundSplit, f64)> = splits
            .into_iter()
            .map(|s| {
                let score = self.score_split(&s.parts);
                (s, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Like [`split_compound`] but each result is paired with a score
    /// and sorted best-first. The score rewards:
    /// - longer minimum-part length (suppresses 3-letter sub-word noise),
    /// - parts that are themselves noun lemmas (suppresses
    ///   "Bunde + Stag" where Bunde is just a Dat Sg of Bund),
    /// - fewer total parts (a 2-part split is usually right for German).
    ///
    /// Scoring is heuristic, not corpus-frequency-based. False positives
    /// are demoted, not eliminated; callers wanting only the top split
    /// should take the first element.
    pub fn split_compound_ranked(&self, surface: &str) -> Vec<(Vec<String>, f64)> {
        let splits: Vec<Vec<String>> = self
            .split_compound_detailed(surface)
            .into_iter()
            .map(|s: CompoundSplit| s.parts)
            .collect();

        let mut scored: Vec<(Vec<String>, f64)> = splits
            .into_iter()
            .map(|s: Vec<String>| {
                let score = self.score_split(&s);
                (s, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    fn score_split(&self, parts: &[String]) -> f64 {
        if parts.is_empty() {
            return 0.0;
        }
        let mut score = 0.0;
        let lens: Vec<usize> = parts.iter().map(|s| s.chars().count()).collect();
        let total_len: usize = lens.iter().sum();
        let min_len = *lens.iter().min().unwrap_or(&0) as f64;
        let n_parts = parts.len() as f64;

        // Base: total surface coverage.
        score += total_len as f64;
        // Bonus: min part is long — discourages "Hau"-style 3-letter
        // false positives.
        score += min_len * 3.0;
        // Bonus per noun-lemma part. A compound is almost always
        // <noun>+<noun>, with occasional non-noun heads.
        for part in parts {
            if self.is_lemma_of_pos(part, UPOS::NOUN) {
                score += 5.0;
            }
        }
        // Penalty for extra parts beyond 2. We charge 8 per extra
        // part: enough that a 3-part split (Bun+Des+Tag) doesn't
        // outrank the canonical 2-part (Bund+Tag) of "Bundestag".
        // Long compounds (Donaudampfschifffahrt) compensate via the
        // total-length and min-length bonuses, so legitimate 3-/4-
        // part decompositions still rank competitively when the parts
        // are long.
        score -= (n_parts - 2.0).max(0.0) * 8.0;
        score
    }

    /// Look up all analyses of a surface form. Returns an empty vector
    /// for unknown surfaces.
    pub fn analyze(&self, surface: &str) -> Vec<Analysis> {
        let packed = match self.fst.get(surface.as_bytes()) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let (count, abs_offset) = unpack_fst_value(packed);
        let mut out = Vec::with_capacity(count as usize);
        let start = abs_offset as usize;
        let end = start + (count as usize) * ANALYSIS_RECORD_SIZE;
        if end > self.analyses_end {
            // Corrupt index — return empty rather than panic.
            return out;
        }
        let records = &self.side[start..end];
        for chunk in records.chunks_exact(ANALYSIS_RECORD_SIZE) {
            let rec = AnalysisRecord::from_bytes(chunk);
            match self.materialise(&rec) {
                Ok(a) => out.push(a),
                Err(_) => continue,
            }
        }
        out
    }

    /// Convert an on-disk record to an [`Analysis`]: resolve its shape
    /// from the shape table, then look up the lemma from the intern
    /// table.
    fn materialise(&self, rec: &AnalysisRecord) -> Result<Analysis, LoadError> {
        let shape = self
            .shapes
            .get(rec.shape_id as usize)
            .ok_or(LoadError::InvalidShape(rec.shape_id))?;
        let pos = match shape.pos {
            0 => UPOS::NOUN,
            1 => UPOS::VERB,
            2 => UPOS::ADJ,
            3 => UPOS::ADV,
            4 => UPOS::PRON,
            5 => UPOS::DET,
            6 => UPOS::NUM,
            7 => UPOS::ADP,
            8 => UPOS::CCONJ,
            9 => UPOS::SCONJ,
            10 => UPOS::AUX,
            11 => UPOS::PART,
            12 => UPOS::INTJ,
            13 => UPOS::PUNCT,
            14 => UPOS::SYM,
            15 => UPOS::X,
            16 => UPOS::PROPN,
            other => return Err(LoadError::InvalidPos(other)),
        };
        let source = match shape.source {
            0 => Source::Attested,
            1 => Source::Inflected,
            2 => Source::Composed,
            3 => Source::Predicted,
            other => return Err(LoadError::InvalidSource(other)),
        };
        let mut features = PackedFeatures(shape.packed_features).unpack();
        // `aux` rides in the shape entry, not the PackedFeatures word.
        features.aux = Aux::from_code(shape.aux);
        let lemma = self.lemma(rec.lemma_id)?;
        Ok(Analysis::with_source(lemma, pos, features, source))
    }

    /// Read lemma N from the intern table.
    fn lemma(&self, id: u32) -> Result<String, LoadError> {
        let id = id as usize;
        if id >= self.num_lemmas {
            return Err(LoadError::Truncated {
                field: "lemma id out of range",
            });
        }
        let off_start = self.lemma_offsets_offset + id * 4;
        let off_end = off_start + 4;
        let start = u32::from_le_bytes(self.side[off_start..off_end].try_into().unwrap()) as usize;
        let next_off_start = off_end;
        let next_off_end = next_off_start + 4;
        let end = u32::from_le_bytes(self.side[next_off_start..next_off_end].try_into().unwrap())
            as usize;
        let bytes_start = self.lemma_bytes_offset + start;
        let bytes_end = self.lemma_bytes_offset + end;
        let slice = &self.side[bytes_start..bytes_end];
        std::str::from_utf8(slice)
            .map(str::to_string)
            .map_err(|_| LoadError::InvalidLemmaUtf8)
    }
}

/// Capitalise the first character of a string (Unicode-aware).
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Lowercase the first character of a string (Unicode-aware). Used by
/// the compound splitter to attempt adjective-headed compound matching
/// where the lemma is lowercase (`steil`) but the surface presents it
/// capitalized (`Steilküste`).
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Case, Features, Gender, Source};
    use crate::lexicon::build::LexiconBuilder;

    /// Test-local helper: project `split_compound_detailed` down to the
    /// parts-only shape that the test assertions were written against.
    /// The public API exposes `split_compound_detailed` (with linkers)
    /// and `split_compound_ranked` (parts with scores); plain parts-only
    /// is only useful inside the test suite.
    fn split_compound(lex: &Lexicon, surface: &str) -> Vec<Vec<String>> {
        lex.split_compound_detailed(surface)
            .into_iter()
            .map(|s| s.parts)
            .collect()
    }

    fn build_two_word_lexicon() -> Lexicon {
        let mut b = LexiconBuilder::new();
        // Tisch paradigm (partial).
        for (sur, case, num) in [
            ("Tisch", Case::Nom, Number::Sg),
            ("Tisch", Case::Dat, Number::Sg),
            ("Tisches", Case::Gen, Number::Sg),
            ("Tische", Case::Nom, Number::Pl),
            ("Tischen", Case::Dat, Number::Pl),
        ] {
            b.add(
                sur,
                "Tisch",
                UPOS::NOUN,
                Features::noun_form(Gender::Masc, num, case),
                Source::Attested,
            )
            .unwrap();
        }
        // Frau paradigm (partial).
        for (sur, case, num) in [
            ("Frau", Case::Nom, Number::Sg),
            ("Frau", Case::Dat, Number::Sg),
            ("Frauen", Case::Nom, Number::Pl),
            ("Frauen", Case::Dat, Number::Pl),
        ] {
            b.add(
                sur,
                "Frau",
                UPOS::NOUN,
                Features::noun_form(Gender::Fem, num, case),
                Source::Attested,
            )
            .unwrap();
        }
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        Lexicon::from_bytes(fst, side).unwrap()
    }

    #[test]
    fn analyze_known_surface_returns_all_analyses() {
        let lex = build_two_word_lexicon();
        let tisch = lex.analyze("Tisch");
        assert_eq!(tisch.len(), 2);
        assert!(tisch
            .iter()
            .all(|a| a.lemma == "Tisch" && a.pos == UPOS::NOUN));
        let cases: Vec<_> = tisch.iter().map(|a| a.features.case).collect();
        assert!(cases.contains(&Some(Case::Nom)));
        assert!(cases.contains(&Some(Case::Dat)));
    }

    #[test]
    fn analyze_unknown_returns_empty() {
        let lex = build_two_word_lexicon();
        assert!(lex.analyze("Quitsch").is_empty());
    }

    #[test]
    fn distinct_lemmas_in_same_lexicon() {
        let lex = build_two_word_lexicon();
        let frau = lex.analyze("Frauen");
        assert_eq!(frau.len(), 2);
        assert!(frau.iter().all(|a| a.lemma == "Frau"));
    }

    #[test]
    fn split_compound_two_parts() {
        // Build a tiny lexicon with two known nouns.
        let mut b = LexiconBuilder::new();
        for surface in &["Lehrer", "Zimmer", "Buch", "Handlung"] {
            b.add(
                surface,
                surface,
                UPOS::NOUN,
                Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
                Source::Attested,
            )
            .unwrap();
        }
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        let splits = split_compound(&lex, "Lehrerzimmer");
        assert!(
            splits
                .iter()
                .any(|p| p == &vec!["Lehrer".to_string(), "Zimmer".to_string()]),
            "missing Lehrer/Zimmer split in {splits:?}"
        );

        let splits = split_compound(&lex, "Buchhandlung");
        assert!(
            splits
                .iter()
                .any(|p| p == &vec!["Buch".to_string(), "Handlung".to_string()]),
            "missing Buch/Handlung split in {splits:?}"
        );
    }

    #[test]
    fn split_compound_with_linker_s() {
        // "Bundestag" = "Bund" + "es" + "Tag". The -es- linker is only
        // accepted if "Bundes" is in the lexicon as an inflected form
        // of "Bund" — model that by adding both Nom Sg and Gen Sg.
        let mut b = LexiconBuilder::new();
        for surface in &["Bund", "Tag"] {
            b.add(
                surface,
                surface,
                UPOS::NOUN,
                Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
                Source::Attested,
            )
            .unwrap();
        }
        // Gen Sg of Bund — required for the -es- linker validity check.
        b.add(
            "Bundes",
            "Bund",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Gen),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        let splits = split_compound(&lex, "Bundestag");
        assert!(
            splits
                .iter()
                .any(|p| p == &vec!["Bund".to_string(), "Tag".to_string()]),
            "missing Bund/Tag split with -es- linker in {splits:?}"
        );
    }

    #[test]
    fn split_compound_rejects_invalid_linker() {
        // Build a lexicon with Hau and Arbeit (both as Noun lemmas) but
        // NO inflected form Haus-of-Hau. The splitter must reject
        // "Hau + s + Arbeit" because Haus is in the lexicon only as
        // its own lemma, not as a form of Hau.
        let mut b = LexiconBuilder::new();
        for surface in &["Hau", "Arbeit", "Haus"] {
            b.add(
                surface,
                surface,
                UPOS::NOUN,
                Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
                Source::Attested,
            )
            .unwrap();
        }
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        let splits = split_compound(&lex, "Hausarbeit");
        // Only Haus + Arbeit is valid; Hau + s + Arbeit must NOT appear.
        assert!(
            splits
                .iter()
                .any(|p| p == &vec!["Haus".to_string(), "Arbeit".to_string()]),
            "missing Haus+Arbeit in {splits:?}"
        );
        assert!(
            !splits
                .iter()
                .any(|p| p == &vec!["Hau".to_string(), "Arbeit".to_string()]),
            "spurious Hau+Arbeit (via invalid -s- linker) in {splits:?}"
        );
    }

    #[test]
    fn split_compound_rejects_dat_sg_as_left() {
        // Build a lexicon with Bund (Nom Sg AND Dat Sg as "Bunde") and
        // Stag as a noun lemma. With the strict rules, "Bunde + Stag"
        // must be rejected because "Bunde" is only Dat Sg, not Gen or
        // Pl.
        let mut b = LexiconBuilder::new();
        b.add(
            "Bund",
            "Bund",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Bunde",
            "Bund",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Dat),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Stag",
            "Stag",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        let splits = split_compound(&lex, "Bundestag");
        assert!(
            !splits
                .iter()
                .any(|p| p == &vec!["Bunde".to_string(), "Stag".to_string()]),
            "spurious Bunde+Stag (Dat Sg used as compound left) in {splits:?}"
        );
    }

    #[test]
    fn split_compound_accepts_gen_sg_as_left() {
        // Bundes is Gen Sg of Bund — accept as compound left.
        let mut b = LexiconBuilder::new();
        b.add(
            "Bund",
            "Bund",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Bundes",
            "Bund",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Gen),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Tag",
            "Tag",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        let splits = split_compound(&lex, "Bundestag");
        // Bundes + Tag (Bundes as Gen Sg used directly) should be a
        // valid split, in addition to Bund + es + Tag.
        assert!(
            splits
                .iter()
                .any(|p| p == &vec!["Bundes".to_string(), "Tag".to_string()])
                || splits
                    .iter()
                    .any(|p| p == &vec!["Bund".to_string(), "Tag".to_string()]),
            "missing valid Bund/Bundes + Tag split in {splits:?}"
        );
    }

    #[test]
    fn split_compound_reassembly_invariant() {
        // THE PROPERTY THAT ACTUALLY MATTERS: every emitted split must
        // reassemble (parts + linkers concatenated, in order) to the
        // exact surface that was queried. If this holds for all
        // splits across realistic inputs, we know the splitter never
        // emits a result like ["Bund", "Tag"] for "Bundestag" without
        // a corresponding "es" linker — the parts-list-only view
        // (which is what the old tests asserted on) was ambiguous and
        // didn't catch that class of bug.
        let mut b = LexiconBuilder::new();
        let entries = [
            // (surface, lemma, gender, number, case)
            ("Bund", "Bund", Gender::Masc, Number::Sg, Case::Nom),
            ("Bundes", "Bund", Gender::Masc, Number::Sg, Case::Gen),
            ("Bunde", "Bund", Gender::Masc, Number::Sg, Case::Dat),
            ("Tag", "Tag", Gender::Masc, Number::Sg, Case::Nom),
            ("Tages", "Tag", Gender::Masc, Number::Sg, Case::Gen),
            ("Stag", "Stag", Gender::Masc, Number::Sg, Case::Nom),
            ("Haus", "Haus", Gender::Neut, Number::Sg, Case::Nom),
            ("Hau", "Hau", Gender::Masc, Number::Sg, Case::Nom),
            ("Arbeit", "Arbeit", Gender::Fem, Number::Sg, Case::Nom),
            ("Wort", "Wort", Gender::Neut, Number::Sg, Case::Nom),
            ("Wörter", "Wort", Gender::Neut, Number::Pl, Case::Nom),
            ("Buch", "Buch", Gender::Neut, Number::Sg, Case::Nom),
            ("Sonne", "Sonne", Gender::Fem, Number::Sg, Case::Nom),
            ("Sonnen", "Sonne", Gender::Fem, Number::Pl, Case::Nom),
            ("Strahl", "Strahl", Gender::Masc, Number::Sg, Case::Nom),
        ];
        for (s, l, g, n, c) in entries {
            b.add(
                s,
                l,
                UPOS::NOUN,
                Features::noun_form(g, n, c),
                Source::Attested,
            )
            .unwrap();
        }
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        for surface in &[
            "Bundestag",
            "Hausarbeit",
            "Wörterbuch",
            "Sonnenstrahl",
            "Tageszeitung",
        ] {
            let splits = lex.split_compound_detailed(surface);
            for split in &splits {
                let reassembled = split.reassemble();
                assert_eq!(
                    &reassembled, *surface,
                    "split {:?} reassembles to {:?}, not {:?}",
                    split, reassembled, surface
                );
                // Length invariant: linkers count is parts count - 1.
                assert_eq!(
                    split.linkers.len() + 1,
                    split.parts.len(),
                    "split has malformed linker/parts arity: {:?}",
                    split
                );
            }
        }
    }

    #[test]
    fn split_compound_bundestag_uses_es_linker() {
        // The CORRECT decomposition of "Bundestag" must use the -es-
        // linker, NOT the empty linker (which would give "Bundtag",
        // not a word). The previous parts-only test couldn't
        // distinguish these.
        let mut b = LexiconBuilder::new();
        b.add(
            "Bund",
            "Bund",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Bundes",
            "Bund",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Gen),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Tag",
            "Tag",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        let splits = lex.split_compound_detailed("Bundestag");
        // Find the Bund+Tag split (parts) and verify the linker is "es",
        // not "".
        let bund_tag = splits
            .iter()
            .find(|s| s.parts == vec!["Bund".to_string(), "Tag".to_string()]);
        let bund_tag = bund_tag.expect("missing Bund+Tag split");
        assert_eq!(
            bund_tag.linkers,
            vec!["es".to_string()],
            "Bundestag should split as Bund + es + Tag, not Bund + '' + Tag"
        );
        assert_eq!(bund_tag.reassemble(), "Bundestag");
    }

    #[test]
    fn split_compound_accepts_pl_as_left() {
        // "Wörterbuch" — Wörter is Pl of Wort, valid compound-left form.
        let mut b = LexiconBuilder::new();
        b.add(
            "Wort",
            "Wort",
            UPOS::NOUN,
            Features::noun_form(Gender::Neut, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Wörter",
            "Wort",
            UPOS::NOUN,
            Features::noun_form(Gender::Neut, Number::Pl, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Buch",
            "Buch",
            UPOS::NOUN,
            Features::noun_form(Gender::Neut, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        let splits = split_compound(&lex, "Wörterbuch");
        assert!(
            splits
                .iter()
                .any(|p| p == &vec!["Wörter".to_string(), "Buch".to_string()]),
            "missing Wörter+Buch split in {splits:?}"
        );
    }

    #[test]
    fn split_compound_three_parts_recursive() {
        let mut b = LexiconBuilder::new();
        for surface in &["Buch", "Hand", "Lung", "Handlung", "Hand"] {
            b.add(
                surface,
                surface,
                UPOS::NOUN,
                Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
                Source::Attested,
            )
            .unwrap();
        }
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        // "Buchhandlung" can split as 2 parts (Buch/Handlung) AND as 3
        // (Buch/Hand/Lung). The recursive splitter returns both.
        let splits = split_compound(&lex, "Buchhandlung");
        let has_two = splits
            .iter()
            .any(|p| p == &vec!["Buch".to_string(), "Handlung".to_string()]);
        assert!(has_two);
    }

    #[test]
    fn split_compound_returns_empty_for_non_compound() {
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
        let lex = Lexicon::from_bytes(fst, side).unwrap();

        let splits = split_compound(&lex, "Tisch");
        assert!(splits.is_empty(), "unexpected splits {splits:?}");
    }

    #[test]
    fn umlaut_lemmas_roundtrip_intern_table() {
        let mut b = LexiconBuilder::new();
        b.add(
            "Bücher",
            "Buch",
            UPOS::NOUN,
            Features::noun_form(Gender::Neut, Number::Pl, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Büchern",
            "Buch",
            UPOS::NOUN,
            Features::noun_form(Gender::Neut, Number::Pl, Case::Dat),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        let lex = Lexicon::from_bytes(fst, side).unwrap();
        let pl = lex.analyze("Bücher");
        assert_eq!(pl.len(), 1);
        assert_eq!(pl[0].lemma, "Buch");
    }
}
