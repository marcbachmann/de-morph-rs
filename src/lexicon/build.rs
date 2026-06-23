//! Build a lexicon from `(surface, lemma, pos, features, source)` records.
//!
//! Two artefacts are written:
//! - the FST file (surface → packed u64 pointer)
//! - the side-table file (header + lemma intern table + analyses)
//!
//! Records do not need to be sorted in input order; the builder sorts
//! by surface internally to satisfy the FST builder's contract. For
//! very large inputs an external sort would be needed; at ~1.4M
//! records (the nouns + verbs lexicon) the in-memory sort fits
//! comfortably and finishes in under a second.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use fst::MapBuilder;

use crate::analysis::{Aux, Features, PackedFeatures, Source, UPOS};
use crate::lexicon::format::{
    bit_width, pack_fst_value, BitWriter, Shape, HEADER_SIZE, MAGIC, MAX_SHAPE_ID, SHAPE_ENTRY_SIZE,
    VERSION_MAJOR, VERSION_MINOR,
};

/// Error returned by [`LexiconBuilder::finish`].
#[derive(Debug)]
pub enum BuildError {
    /// I/O error while writing one of the output streams.
    Io(std::io::Error),
    /// The FST builder rejected an entry (typically: keys not sorted).
    Fst(fst::Error),
    /// More than `u32::MAX` lemmas — out-of-range for the format.
    TooManyLemmas,
    /// More than `u32::MAX` analyses — out-of-range for the format.
    TooManyAnalyses,
    /// A single surface has more than `u32::MAX` analyses.
    TooManyAnalysesPerSurface,
    /// More distinct analysis shapes than the 12-bit `shape_id` allows.
    TooManyShapes,
    /// Side-table size exceeds `u32::MAX` bytes.
    SideTableTooLarge,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Fst(e) => write!(f, "fst error: {e}"),
            Self::TooManyLemmas => write!(f, "more than u32::MAX lemmas"),
            Self::TooManyAnalyses => write!(f, "more than u32::MAX analyses"),
            Self::TooManyAnalysesPerSurface => {
                write!(f, "more than u32::MAX analyses for one surface")
            }
            Self::TooManyShapes => write!(f, "more than 4096 distinct analysis shapes"),
            Self::SideTableTooLarge => write!(f, "side table exceeds 4 GiB"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<fst::Error> for BuildError {
    fn from(e: fst::Error) -> Self {
        Self::Fst(e)
    }
}

/// Whether a surface belongs in the analyzer FST. Rejects:
///
/// - **Any whitespace.** The analyzer is keyed on single whitespace
///   tokens, so a surface containing a space is multi-token and can
///   never match a single input token: `zu lieben` (look up `zu` and
///   `lieben` separately), multiword lemmas (`Sicherheitsrat der
///   Vereinten Nationen`), `so genannte`, the separated forms of
///   separable verbs (`tauche ab`). The single-word zu-infinitive of a
///   separable verb (`abzutauchen`) has no space and is kept.
/// - **Leaked wikitext/HTML markup** the template parser passed through:
///   HTML comments (`isometrischer <!--…-->`), `<small>`/`<ref>` tags,
///   doubled braces/brackets.
///
/// Single-bracket punctuation entries (`[`, `]`, `{`, `}`) are kept —
/// only *doubled* braces/brackets signal markup.
pub fn is_clean_surface(surface: &str) -> bool {
    !(surface.contains(char::is_whitespace)
        || surface.contains('<')
        || surface.contains('>')
        || surface.contains("{{")
        || surface.contains("}}")
        || surface.contains("[[")
        || surface.contains("]]"))
}

/// Streaming builder. Records are added in arbitrary order; sorting
/// and FST/side-table emission happens in [`LexiconBuilder::finish`].
#[derive(Default)]
pub struct LexiconBuilder {
    /// Collected records grouped by surface form for sorting.
    by_surface: BTreeMap<String, Vec<PendingRecord>>,
    /// Lemma intern table: lemma string → lemma_id.
    lemma_ids: HashMap<String, u32>,
    /// Lemmas in insertion order so we can write them sequentially.
    lemmas: Vec<String>,
    total_records: u64,
}

#[derive(Debug, Clone)]
struct PendingRecord {
    lemma_id: u32,
    pos: UPOS,
    source: Source,
    features: PackedFeatures,
    /// Auxiliary code (0=unset/1=Haben/2=Sein/3=Both) — carried
    /// separately because `PackedFeatures` is full and can't hold it.
    aux: u8,
}

impl LexiconBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn total_records(&self) -> u64 {
        self.total_records
    }

    pub fn num_lemmas(&self) -> usize {
        self.lemmas.len()
    }

    pub fn num_surfaces(&self) -> usize {
        self.by_surface.len()
    }

    /// Add one record to the lexicon.
    ///
    /// The `surface` is the FST key; `lemma` is interned into the
    /// shared table on first encounter. Identical (surface, lemma,
    /// pos, features, source) tuples are de-duplicated.
    pub fn add(
        &mut self,
        surface: &str,
        lemma: &str,
        pos: UPOS,
        features: Features,
        source: Source,
    ) -> Result<(), BuildError> {
        let lemma_id = match self.lemma_ids.get(lemma) {
            Some(&id) => id,
            None => {
                let id = u32::try_from(self.lemmas.len()).map_err(|_| BuildError::TooManyLemmas)?;
                self.lemmas.push(lemma.to_string());
                self.lemma_ids.insert(lemma.to_string(), id);
                id
            }
        };
        let rec = PendingRecord {
            lemma_id,
            pos,
            source,
            features: PackedFeatures::pack(features),
            aux: Aux::to_code(features.aux),
        };
        let bucket = self.by_surface.entry(surface.to_string()).or_default();
        if !bucket.iter().any(|r| {
            r.lemma_id == rec.lemma_id
                && r.pos == rec.pos
                && r.source == rec.source
                && r.features == rec.features
                && r.aux == rec.aux
        }) {
            bucket.push(rec);
            self.total_records += 1;
        }
        Ok(())
    }

    /// Finalise the lexicon. Writes the FST to `fst_out` and the side
    /// table to `side_out`. Both writers must be empty on entry.
    pub fn finish<W1: Write, W2: Write>(
        self,
        mut fst_out: W1,
        mut side_out: W2,
    ) -> Result<BuildStats, BuildError> {
        let LexiconBuilder {
            by_surface,
            lemmas,
            total_records,
            ..
        } = self;

        let num_lemmas = u32::try_from(lemmas.len()).map_err(|_| BuildError::TooManyLemmas)?;

        // ---------------- Side table -----------------------------------------
        // Layout:
        //   header [HEADER_SIZE bytes]
        //   lemma_offsets [(num_lemmas + 1) * 4 bytes]
        //   lemma_bytes
        //   shape_table [num_shapes * SHAPE_ENTRY_SIZE bytes]
        //   analyses (bit-packed, lemma-factored groups; see format.rs)

        // Pass 1: intern each record's (pos, source, aux, features) tuple
        // into the shape table and collect every surface's readings as
        // (lemma_id, shape_id) pairs, sorted by lemma then shape so equal
        // lemmas are contiguous (order within a surface is not
        // semantically meaningful — ingest order is already arbitrary).
        let mut shape_ids: HashMap<Shape, u16> = HashMap::new();
        let mut shapes: Vec<Shape> = Vec::new();
        let mut per_surface: Vec<(String, Vec<(u32, u16)>)> = Vec::with_capacity(by_surface.len());
        for (surface, records) in by_surface {
            let mut readings: Vec<(u32, u16)> = Vec::with_capacity(records.len());
            for rec in records {
                let shape = Shape {
                    packed_features: rec.features.0,
                    pos: rec.pos as u8,
                    source: rec.source as u8,
                    aux: rec.aux,
                };
                let shape_id = match shape_ids.get(&shape) {
                    Some(&id) => id,
                    None => {
                        if shapes.len() as u32 > MAX_SHAPE_ID {
                            return Err(BuildError::TooManyShapes);
                        }
                        let id = shapes.len() as u16;
                        shapes.push(shape);
                        shape_ids.insert(shape, id);
                        id
                    }
                };
                readings.push((rec.lemma_id, shape_id));
            }
            readings.sort_unstable();
            // Defensive: `add` already dedups exact (lemma, shape, ...)
            // tuples, which map 1:1 onto (lemma_id, shape_id) pairs.
            readings.dedup();
            per_surface.push((surface, readings));
        }
        let num_shapes = u32::try_from(shapes.len()).map_err(|_| BuildError::TooManyShapes)?;

        // Field widths derived from table sizes, recorded in the header so
        // the loader unpacks identically.
        let lemma_bits = bit_width(num_lemmas as usize);
        let shape_bits = bit_width(shapes.len());

        // Pass 2: bit-pack each surface's run (byte-aligned), factoring
        // the lemma out — write lemma_id once per distinct lemma, marked
        // by a 1-bit `new_lemma` flag on every reading after the first.
        let mut groups = BitWriter::new();
        let mut surface_spans: Vec<(String, u32, u64)> = Vec::with_capacity(per_surface.len());
        let mut total_readings: u64 = 0;
        for (surface, readings) in per_surface {
            let count =
                u32::try_from(readings.len()).map_err(|_| BuildError::TooManyAnalysesPerSurface)?;
            let offset_in_analyses = groups.byte_len() as u64;
            let mut prev_lemma = u32::MAX; // never a valid lemma_id (< 2^20)
            for (i, &(lemma_id, shape_id)) in readings.iter().enumerate() {
                if i == 0 {
                    groups.write(lemma_id, lemma_bits);
                } else if lemma_id == prev_lemma {
                    groups.write(0, 1);
                } else {
                    groups.write(1, 1);
                    groups.write(lemma_id, lemma_bits);
                }
                groups.write(shape_id as u32, shape_bits);
                prev_lemma = lemma_id;
            }
            groups.align();
            total_readings += count as u64;
            surface_spans.push((surface, count, offset_in_analyses));
        }
        let analyses_bytes = groups.into_bytes();

        let lemma_offsets_offset = HEADER_SIZE as u64;
        let lemma_offsets_bytes = (num_lemmas as u64 + 1) * 4;
        let lemma_bytes_offset = lemma_offsets_offset + lemma_offsets_bytes;
        let lemma_total_bytes: u64 = lemmas.iter().map(|s| s.len() as u64).sum();
        let shape_table_offset = lemma_bytes_offset + lemma_total_bytes;
        let shape_table_bytes = num_shapes as u64 * SHAPE_ENTRY_SIZE as u64;
        let analyses_offset = shape_table_offset + shape_table_bytes;

        let num_analyses =
            u32::try_from(total_readings).map_err(|_| BuildError::TooManyAnalyses)?;
        let analyses_end = analyses_offset + analyses_bytes.len() as u64;
        let analyses_end_u32 =
            u32::try_from(analyses_end).map_err(|_| BuildError::SideTableTooLarge)?;
        let lemma_offsets_offset_u32 = lemma_offsets_offset as u32;
        let lemma_bytes_offset_u32 = lemma_bytes_offset as u32;
        let shape_table_offset_u32 =
            u32::try_from(shape_table_offset).map_err(|_| BuildError::SideTableTooLarge)?;
        let analyses_offset_u32 =
            u32::try_from(analyses_offset).map_err(|_| BuildError::SideTableTooLarge)?;

        // Resolve per-surface absolute analysis offsets now that the
        // shape table size is known. `surface_spans` preserves the
        // BTreeMap's lexicographic order, so FST insertion stays sorted.
        let mut fst_entries: Vec<(String, u64)> = Vec::with_capacity(surface_spans.len());
        for (surface, count, offset_in_analyses) in surface_spans {
            let absolute_offset = analyses_offset + offset_in_analyses;
            let absolute_offset_u32 =
                u32::try_from(absolute_offset).map_err(|_| BuildError::SideTableTooLarge)?;
            fst_entries.push((surface, pack_fst_value(count, absolute_offset_u32)));
        }

        // Build the FST.
        let mut fst_builder = MapBuilder::new(&mut fst_out)?;
        for (surface, value) in &fst_entries {
            fst_builder.insert(surface, *value)?;
        }
        fst_builder.finish()?;

        // ---------------- Write the side table ------------------------------
        // Header (64 bytes, all little-endian).
        let mut header = [0u8; HEADER_SIZE];
        header[0..12].copy_from_slice(&MAGIC);
        header[12..14].copy_from_slice(&VERSION_MAJOR.to_le_bytes());
        header[14..16].copy_from_slice(&VERSION_MINOR.to_le_bytes());
        header[16..20].copy_from_slice(&0u32.to_le_bytes()); // flags
        header[20..24].copy_from_slice(&num_lemmas.to_le_bytes());
        header[24..28].copy_from_slice(&num_analyses.to_le_bytes());
        header[28..32].copy_from_slice(&lemma_offsets_offset_u32.to_le_bytes());
        header[32..36].copy_from_slice(&lemma_bytes_offset_u32.to_le_bytes());
        header[36..40].copy_from_slice(&analyses_offset_u32.to_le_bytes());
        header[40..44].copy_from_slice(&analyses_end_u32.to_le_bytes());
        header[44..48].copy_from_slice(&num_shapes.to_le_bytes());
        header[48..52].copy_from_slice(&shape_table_offset_u32.to_le_bytes());
        header[52] = lemma_bits as u8;
        header[53] = shape_bits as u8;
        // bytes 54..64 are reserved (already zeroed).
        side_out.write_all(&header)?;

        // Lemma offsets: (num_lemmas + 1) * u32, each relative to
        // lemma_bytes_offset.
        let mut running: u32 = 0;
        for lemma in &lemmas {
            side_out.write_all(&running.to_le_bytes())?;
            running = running
                .checked_add(lemma.len() as u32)
                .ok_or(BuildError::SideTableTooLarge)?;
        }
        // Sentinel for length of the last lemma.
        side_out.write_all(&running.to_le_bytes())?;

        // Lemma bytes.
        for lemma in &lemmas {
            side_out.write_all(lemma.as_bytes())?;
        }

        // Shape table.
        for shape in &shapes {
            side_out.write_all(&shape.to_bytes())?;
        }

        // Analyses.
        side_out.write_all(&analyses_bytes)?;
        side_out.flush()?;

        Ok(BuildStats {
            num_lemmas: num_lemmas as u64,
            num_analyses: num_analyses as u64,
            num_surfaces: fst_entries.len() as u64,
            total_records,
            side_table_bytes: analyses_end,
        })
    }
}

/// Statistics returned from [`LexiconBuilder::finish`].
#[derive(Debug, Clone, Copy)]
pub struct BuildStats {
    pub num_lemmas: u64,
    pub num_analyses: u64,
    pub num_surfaces: u64,
    pub total_records: u64,
    pub side_table_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Case, Gender, Number};
    use crate::lexicon::load::Lexicon;

    #[test]
    fn is_clean_surface_keeps_single_tokens_and_rejects_multitoken_and_markup() {
        // Single-token surfaces — incl. word-internal compounds, the
        // single-word zu-infinitive of separable verbs, and the
        // single-bracket punctuation entries.
        for ok in [
            "Tisch",
            "groß",
            "stilllegen",
            "abzutauchen",
            "[",
            "]",
            "{",
            "}",
            "...",
        ] {
            assert!(is_clean_surface(ok), "wrongly rejected {ok:?}");
        }
        // Multi-token surfaces (any space) — not analysable as one token.
        for bad in [
            "zu lieben",
            "so genannte",
            "Sicherheitsrat der Vereinten Nationen",
            "tauche ab",
        ] {
            assert!(
                !is_clean_surface(bad),
                "failed to reject multi-token {bad:?}"
            );
        }
        // Contaminated surfaces from leaked template markup / whitespace.
        for bad in [
            "isometrischer      <!--laut Duden keine Steigerung-->er",
            "<small>(schneibte)</small>",
            "-ige\n<!--",
            "Foo{{x}}",
            "a[[b]]",
            "x\ty",
        ] {
            assert!(!is_clean_surface(bad), "failed to reject {bad:?}");
        }
    }

    #[test]
    fn build_and_load_roundtrip_minimal() {
        let mut b = LexiconBuilder::new();
        b.add(
            "Tisch",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Tisch",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Dat),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Tische",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Pl, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Tischen",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Pl, Case::Dat),
            Source::Attested,
        )
        .unwrap();

        let mut fst_bytes = Vec::new();
        let mut side_bytes = Vec::new();
        let stats = b.finish(&mut fst_bytes, &mut side_bytes).unwrap();
        assert_eq!(stats.num_lemmas, 1);
        assert_eq!(stats.num_analyses, 4);
        assert_eq!(stats.num_surfaces, 3);

        let lex = Lexicon::from_bytes(fst_bytes, side_bytes).unwrap();
        let analyses = lex.analyze("Tisch");
        assert_eq!(analyses.len(), 2);
        assert!(analyses.iter().all(|a| a.lemma == "Tisch"));

        let datives = lex.analyze("Tischen");
        assert_eq!(datives.len(), 1);
        let a = &datives[0];
        assert_eq!(a.lemma, "Tisch");
        assert_eq!(a.features.case, Some(Case::Dat));
        assert_eq!(a.features.number, Some(Number::Pl));

        // Unknown surface returns empty.
        assert!(lex.analyze("Quitsch").is_empty());
    }

    #[test]
    fn single_lemma_and_shape_zero_width_roundtrip() {
        // One lemma and one identical shape across surfaces drive both
        // lemma_bits and shape_bits to 0 — the v6 width-0 codec path.
        let mut b = LexiconBuilder::new();
        let feats = Features::noun(Gender::Masc);
        for surf in ["alpha", "beta", "gamma"] {
            b.add(surf, "x", UPOS::NOUN, feats, Source::Attested).unwrap();
        }
        let mut fst = Vec::new();
        let mut side = Vec::new();
        let stats = b.finish(&mut fst, &mut side).unwrap();
        assert_eq!(stats.num_lemmas, 1);
        assert_eq!(stats.num_surfaces, 3);
        assert_eq!(stats.num_analyses, 3);
        assert_eq!(side[52], 0, "lemma_bits should be 0 for one lemma");
        assert_eq!(side[53], 0, "shape_bits should be 0 for one shape");

        let lex = Lexicon::from_bytes(fst, side).unwrap();
        for surf in ["alpha", "beta", "gamma"] {
            let a = lex.analyze(surf);
            assert_eq!(a.len(), 1, "{surf}");
            assert_eq!(a[0].lemma, "x");
            assert_eq!(a[0].pos, UPOS::NOUN);
            assert_eq!(a[0].features.gender, Some(Gender::Masc));
        }
        assert!(lex.analyze("delta").is_empty());
    }

    #[test]
    fn multi_lemma_surface_toggles_new_lemma_flag() {
        // One surface bearing two different lemmas exercises the
        // new_lemma flag: reading 0 writes a lemma, a later reading flips
        // the flag and re-reads. Readings are lemma-sorted, so the flag
        // toggles 0 (same lemma) then 1 (new lemma).
        let mut b = LexiconBuilder::new();
        b.add("Leiter", "Leiter", UPOS::NOUN, Features::noun(Gender::Fem), Source::Attested)
            .unwrap();
        b.add("Leiter", "Leiter", UPOS::NOUN, Features::noun(Gender::Masc), Source::Attested)
            .unwrap();
        b.add("Leiter", "leiten", UPOS::VERB, Features::empty(), Source::Inflected)
            .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        let stats = b.finish(&mut fst, &mut side).unwrap();
        assert_eq!(stats.num_lemmas, 2);
        assert!(side[52] >= 1, "lemma_bits must be >=1 with two lemmas");

        let lex = Lexicon::from_bytes(fst, side).unwrap();
        let analyses = lex.analyze("Leiter");
        assert_eq!(analyses.len(), 3);
        let lemmas: std::collections::BTreeSet<&str> =
            analyses.iter().map(|a| &*a.lemma).collect();
        assert_eq!(
            lemmas,
            ["Leiter", "leiten"].into_iter().collect::<std::collections::BTreeSet<_>>()
        );
    }

    #[test]
    fn corrupt_width_header_is_rejected() {
        // A stored width inconsistent with the table size must be caught
        // at load rather than silently mis-decoding every record.
        let mut b = LexiconBuilder::new();
        b.add(
            "Tisch",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Tische",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Pl, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        let mut fst = Vec::new();
        let mut side = Vec::new();
        b.finish(&mut fst, &mut side).unwrap();
        side[52] = side[52].wrapping_add(1); // corrupt lemma_bits
        assert!(matches!(
            Lexicon::from_bytes(fst, side),
            Err(crate::lexicon::load::LoadError::CorruptHeader { .. })
        ));
    }
}
