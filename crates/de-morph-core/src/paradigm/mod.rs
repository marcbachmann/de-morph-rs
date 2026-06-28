//! Rule-based paradigm generation for German nouns / verbs / adjectives.
//!
//! Two distinct jobs:
//! - **Generate**: given a known lemma, gender, declension class and (where
//!   relevant) a plural form, produce the full paradigm. This is the
//!   "I know everything about this lemma except the inflected forms"
//!   case. Forms are tagged [`crate::analysis::Source::Inflected`].
//! - **Guess**: given an out-of-vocabulary surface form, heuristically
//!   propose lemma + declension hypotheses ranked by confidence. Forms
//!   produced from these guesses are tagged
//!   [`crate::analysis::Source::Predicted`].
//!
//! The two layers compose: the runtime analyzer first tries the FST
//! lexicon; if that misses, it falls back to the guesser; the `Source`
//! tag on each returned [`crate::analysis::Analysis`] tells the caller
//! how much to trust the result.
//!
//! Each part of speech lives in its own sibling module: `noun`,
//! `verb`, `adjective`, and `closed_class`.

pub mod adjective;
pub mod closed_class;
pub mod noun;
pub mod verb;

pub use adjective::{generate_adjective_paradigm, AdjectiveAttested};
pub use closed_class::generate_closed_class_entries;
pub use noun::{
    default_plural_guess, generate_noun_paradigm, guess_noun, predict_dative_forms, Confidence,
    NounClass, NounGuess,
};
pub use verb::{generate_verb_paradigm, VerbAttested};
