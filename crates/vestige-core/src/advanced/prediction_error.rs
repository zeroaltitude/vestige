//! # Prediction Error Gating
//!
//! Implements neuroscience-inspired prediction error gating for intelligent memory updates.
//!
//! Based on research showing that prediction error (PE) determines whether memories are:
//! - **Updated** (small PE): New info is similar enough to existing memory
//! - **Created** (large PE): New info is different enough to warrant new memory
//!
//! This solves the "bad vs good similar memory" problem by:
//! 1. Detecting when new content is similar to existing memories
//! 2. Calculating the prediction error (semantic distance)
//! 3. Deciding whether to update existing or create new
//! 4. Optionally superseding outdated memories
//!
//! ## Scientific Background
//!
//! Based on:
//! - Sinclair & Bhavnani (2020): "The Reconsolidation Dilemma"
//! - Lee et al. (2017): Prediction error and memory updating
//! - Google Titans (2025): Surprise-based storage
//!
//! ## Example
//!
//! ```rust,ignore
//! use vestige_core::advanced::prediction_error::PredictionErrorGate;
//!
//! let gate = PredictionErrorGate::new();
//!
//! // New content arrives
//! let decision = gate.evaluate(
//!     "Use async/await for better performance",
//!     &existing_memories,
//!     &embeddings,
//! );
//!
//! match decision {
//!     GateDecision::Update { target, .. } => {
//!         // Update existing memory with new info
//!     }
//!     GateDecision::Create { .. } => {
//!         // Create new memory
//!     }
//!     GateDecision::Supersede { old, .. } => {
//!         // New memory supersedes old (demote old)
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ============================================================================
// CONSTANTS
// ============================================================================

/// Default similarity threshold for considering memories as "similar"
/// Above this = potential update candidate
const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.75;

/// Threshold for considering content as "nearly identical"
/// Above this = definitely update, not create
const NEAR_IDENTICAL_THRESHOLD: f32 = 0.92;

/// Threshold for "correction" detection.
///
/// Supersede is the only *destructive* decision this gate can make: the caller
/// (`Storage::smart_ingest`) responds to it by demoting the old node's FSRS
/// strengths, which is unrecorded and not automatically reversible. It must
/// therefore demand **more** evidence than a plain update, not less. Until
/// `openclaw-vestige-1de` this sat at 0.70 — below `DEFAULT_SIMILARITY_THRESHOLD`
/// — so the destructive branch was strictly easier to reach than the safe one.
const CORRECTION_THRESHOLD: f32 = 0.85;

/// Minimum lexical overlap (Jaccard over content words) between the new text
/// and the candidate before *any* supersede may fire.
///
/// Embedding similarity alone cannot tell "this corrects that" apart from
/// "these are two different notes about the same project": two lesson notes on
/// one codebase land around 0.75-0.85 while sharing no facts at all. Concrete
/// token overlap is what separates the two cases.
const MIN_CONTENT_OVERLAP: f32 = 0.25;

/// Minimum token length for a word to count toward lexical overlap.
/// Discards most English function words on its own; [`OVERLAP_STOPWORDS`]
/// catches the longer ones that slip through.
const MIN_OVERLAP_TOKEN_LEN: usize = 4;

/// Maximum candidates to consider for update
const MAX_UPDATE_CANDIDATES: usize = 5;

/// Common 4+ letter fillers that survive `MIN_OVERLAP_TOKEN_LEN` but carry no
/// topical signal, so they would inflate the overlap score.
const OVERLAP_STOPWORDS: &[&str] = &[
    "about", "also", "because", "been", "before", "both", "could", "does", "each", "from", "have",
    "here", "into", "just", "like", "made", "make", "many", "more", "most", "much", "must", "only",
    "other", "over", "same", "should", "some", "such", "than", "that", "their", "them", "then",
    "there", "these", "they", "this", "those", "through", "very", "were", "what", "when", "where",
    "which", "while", "will", "with", "would", "your",
];

/// Explicit antonym pairs. Each counts when one text carries one side and the
/// other text carries the other — a contradiction with no negation word in
/// sight ("this API is deprecated" vs "this API is recommended").
///
/// The pre-`openclaw-vestige-1de` code listed pairs like these but ignored the
/// second element entirely, firing whenever the *new* text alone contained the
/// first one.
const ANTONYM_PAIRS: &[(&str, &str)] = &[
    ("never", "always"),
    ("wrong", "right"),
    ("bad", "good"),
    ("incorrect", "correct"),
    ("deprecated", "recommended"),
    ("outdated", "current"),
];

/// Markers that flip the polarity of a claim.
///
/// One text carrying one of these while the other does not is evidence of a
/// contradiction only when the two texts are about the same subject — hence
/// the lexical-overlap requirement in [`PredictionErrorGate::detect_contradiction`].
/// Without it, "the dream cycle does not run GC" contradicts every note that
/// happens to sit nearby in embedding space.
const NEGATION_MARKERS: &[&str] = &[
    "don't",
    "do not",
    "doesn't",
    "does not",
    "isn't",
    "is not",
    "aren't",
    "are not",
    "won't",
    "will not",
    "shouldn't",
    "should not",
    "cannot",
    "can't",
    "never",
    "avoid",
    "no longer",
    "instead of",
    "rather than",
    "not ",
];

/// Phrases that mark the new text as an explicit edit of something.
///
/// On their own these say nothing about *what* is being edited — "fixed" is in
/// every bug-fix note ever written — so they only count toward a supersede when
/// the two texts also share concrete vocabulary.
const CORRECTION_MARKERS: &[&str] = &[
    "actually",
    "correction",
    "update:",
    "fixed",
    "was wrong",
    "should be",
    "better approach",
    "improved",
    "the right way",
];

// ============================================================================
// GATE DECISION
// ============================================================================

/// Decision made by the prediction error gate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GateDecision {
    /// Create a new memory (high prediction error)
    Create {
        /// Reason for creating new
        reason: CreateReason,
        /// Prediction error score (0.0 = identical, 1.0 = completely different)
        prediction_error: f32,
        /// Related memories that were considered
        related_memory_ids: Vec<String>,
    },

    /// Update an existing memory (low prediction error)
    Update {
        /// ID of memory to update
        target_id: String,
        /// How similar the content is (0.0 - 1.0)
        similarity: f32,
        /// Type of update to perform
        update_type: UpdateType,
        /// Prediction error score
        prediction_error: f32,
    },

    /// Supersede an existing memory (correction/improvement)
    Supersede {
        /// ID of memory being superseded
        old_memory_id: String,
        /// Similarity to old memory
        similarity: f32,
        /// Why this supersedes the old one
        supersede_reason: SupersedeReason,
        /// Prediction error score
        prediction_error: f32,
    },

    /// Merge with multiple existing memories
    Merge {
        /// IDs of memories to merge with
        memory_ids: Vec<String>,
        /// Average similarity
        avg_similarity: f32,
        /// Merge strategy
        strategy: MergeStrategy,
    },
}

impl GateDecision {
    /// Get the prediction error score
    pub fn prediction_error(&self) -> f32 {
        match self {
            Self::Create { prediction_error, .. } => *prediction_error,
            Self::Update { prediction_error, .. } => *prediction_error,
            Self::Supersede { prediction_error, .. } => *prediction_error,
            Self::Merge { avg_similarity, .. } => 1.0 - avg_similarity,
        }
    }

    /// Check if this is a create decision
    pub fn is_create(&self) -> bool {
        matches!(self, Self::Create { .. })
    }

    /// Check if this is an update decision
    pub fn is_update(&self) -> bool {
        matches!(self, Self::Update { .. })
    }

    /// Get target ID if updating or superseding
    pub fn target_id(&self) -> Option<&str> {
        match self {
            Self::Update { target_id, .. } => Some(target_id),
            Self::Supersede { old_memory_id, .. } => Some(old_memory_id),
            _ => None,
        }
    }
}

/// Reasons for creating a new memory
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CreateReason {
    /// No similar memories exist
    NoSimilarMemories,
    /// Content is substantially different from all candidates
    HighPredictionError,
    /// Different domain/topic despite surface similarity
    DifferentDomain,
    /// Explicitly requested new memory (not update)
    ExplicitCreate,
    /// First memory in the system
    FirstMemory,
}

/// Types of updates to existing memories
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UpdateType {
    /// Append new information
    Append,
    /// Replace content entirely
    Replace,
    /// Merge content intelligently
    Merge,
    /// Add as related context
    AddContext,
    /// Strengthen existing memory (same content, reinforcement)
    Reinforce,
}

/// Reasons for superseding an existing memory
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SupersedeReason {
    /// New content is a correction of old
    Correction,
    /// New content is an improvement/update
    Improvement,
    /// Old content is marked as outdated
    Outdated,
    /// User explicitly indicated this is better
    UserIndicated,
    /// New content has higher confidence/authority
    HigherConfidence,
}

/// Strategies for merging multiple memories
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Combine all content
    Combine,
    /// Keep most recent, link to older
    KeepRecent,
    /// Create summary of all
    Summarize,
    /// Create hierarchy (parent with children)
    Hierarchical,
}

// ============================================================================
// CANDIDATE MEMORY
// ============================================================================

/// A candidate memory for update consideration
#[derive(Debug, Clone)]
pub struct CandidateMemory {
    /// Memory ID
    pub id: String,
    /// Memory content
    pub content: String,
    /// Embedding vector
    pub embedding: Vec<f32>,
    /// Current retrieval strength
    pub retrieval_strength: f64,
    /// Current retention strength
    pub retention_strength: f64,
    /// Tags on the memory
    pub tags: Vec<String>,
    /// Source of the memory
    pub source: Option<String>,
    /// Whether this memory was previously demoted
    pub was_demoted: bool,
    /// Whether this memory was previously promoted
    pub was_promoted: bool,
}

/// Result of similarity comparison
#[derive(Debug, Clone)]
pub struct SimilarityResult {
    /// Memory ID
    pub memory_id: String,
    /// Cosine similarity score (0.0 - 1.0)
    pub similarity: f32,
    /// Prediction error (1.0 - similarity)
    pub prediction_error: f32,
    /// Semantic overlap (estimated shared concepts)
    pub semantic_overlap: f32,
    /// Whether contents appear contradictory
    pub appears_contradictory: bool,
}

// ============================================================================
// PREDICTION ERROR GATE
// ============================================================================

/// Configuration for the prediction error gate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionErrorConfig {
    /// Similarity threshold for update consideration
    pub similarity_threshold: f32,
    /// Threshold for near-identical detection
    pub near_identical_threshold: f32,
    /// Threshold for correction detection
    pub correction_threshold: f32,
    /// Minimum lexical overlap between new and old content before a supersede
    /// may fire. Guards against superseding a memory that merely *sounds*
    /// related in embedding space while sharing no actual facts.
    #[serde(default = "default_min_content_overlap")]
    pub min_content_overlap: f32,
    /// Maximum candidates to consider
    pub max_candidates: usize,
    /// Whether to auto-supersede demoted memories
    pub auto_supersede_demoted: bool,
    /// Whether to prefer updates over creates
    pub prefer_updates: bool,
}

fn default_min_content_overlap() -> f32 {
    MIN_CONTENT_OVERLAP
}

impl Default for PredictionErrorConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: DEFAULT_SIMILARITY_THRESHOLD,
            near_identical_threshold: NEAR_IDENTICAL_THRESHOLD,
            correction_threshold: CORRECTION_THRESHOLD,
            min_content_overlap: MIN_CONTENT_OVERLAP,
            max_candidates: MAX_UPDATE_CANDIDATES,
            auto_supersede_demoted: true,
            prefer_updates: true,
        }
    }
}

/// The Prediction Error Gate
///
/// Evaluates new content against existing memories to determine
/// whether to create, update, or supersede.
#[derive(Debug)]
pub struct PredictionErrorGate {
    /// Configuration
    config: PredictionErrorConfig,
    /// Statistics
    stats: GateStats,
}

impl Default for PredictionErrorGate {
    fn default() -> Self {
        Self::new()
    }
}

impl PredictionErrorGate {
    /// Create a new prediction error gate with default config
    pub fn new() -> Self {
        Self {
            config: PredictionErrorConfig::default(),
            stats: GateStats::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(config: PredictionErrorConfig) -> Self {
        Self {
            config,
            stats: GateStats::default(),
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &PredictionErrorConfig {
        &self.config
    }

    /// Get mutable configuration
    pub fn config_mut(&mut self) -> &mut PredictionErrorConfig {
        &mut self.config
    }

    /// Evaluate new content against candidates
    ///
    /// Returns a decision on whether to create, update, or supersede.
    pub fn evaluate(
        &mut self,
        new_content: &str,
        new_embedding: &[f32],
        candidates: &[CandidateMemory],
    ) -> GateDecision {
        self.stats.total_evaluations += 1;

        // No candidates = definitely create
        if candidates.is_empty() {
            self.stats.creates += 1;
            return GateDecision::Create {
                reason: CreateReason::FirstMemory,
                prediction_error: 1.0,
                related_memory_ids: vec![],
            };
        }

        // Calculate similarities
        let mut similarities: Vec<SimilarityResult> = candidates
            .iter()
            .map(|c| {
                let similarity = cosine_similarity(new_embedding, &c.embedding);
                let appears_contradictory = self.detect_contradiction(new_content, &c.content);

                SimilarityResult {
                    memory_id: c.id.clone(),
                    similarity,
                    prediction_error: 1.0 - similarity,
                    semantic_overlap: similarity, // Simplified; could use more sophisticated measure
                    appears_contradictory,
                }
            })
            .collect();

        // Sort by similarity (highest first)
        similarities.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));

        // Take top candidates
        let top_candidates: Vec<_> = similarities
            .iter()
            .take(self.config.max_candidates)
            .collect();

        // Check for near-identical match
        if let Some(best) = top_candidates.first() {
            if best.similarity >= self.config.near_identical_threshold {
                // Nearly identical - reinforce existing
                self.stats.updates += 1;
                return GateDecision::Update {
                    target_id: best.memory_id.clone(),
                    similarity: best.similarity,
                    update_type: UpdateType::Reinforce,
                    prediction_error: best.prediction_error,
                };
            }

            // Check for potential supersede
            let candidate = candidates.iter().find(|c| c.id == best.memory_id);
            if let Some(c) = candidate {
                // Supersede demotes the old node's FSRS strengths and
                // Update{Merge} rewrites its content in place — both conflate
                // two memories into one. Gate all three on the texts actually
                // sharing concrete vocabulary, not merely sitting close in
                // embedding space (openclaw-vestige-1de).
                let overlap = lexical_overlap(new_content, &c.content);
                let overlap_ok = overlap >= self.config.min_content_overlap;

                // If similar and the existing memory was demoted, supersede it
                if best.similarity >= self.config.similarity_threshold
                    && overlap_ok
                    && c.was_demoted
                    && self.config.auto_supersede_demoted
                {
                    self.stats.supersedes += 1;
                    return GateDecision::Supersede {
                        old_memory_id: c.id.clone(),
                        similarity: best.similarity,
                        supersede_reason: SupersedeReason::Improvement,
                        prediction_error: best.prediction_error,
                    };
                }

                // Check for correction: highly similar, overlapping, and either
                // directly contradictory or explicitly marked as an edit.
                if best.similarity >= self.config.correction_threshold
                    && overlap_ok
                    && (best.appears_contradictory || Self::has_correction_marker(new_content))
                {
                    self.stats.supersedes += 1;
                    return GateDecision::Supersede {
                        old_memory_id: c.id.clone(),
                        similarity: best.similarity,
                        supersede_reason: SupersedeReason::Correction,
                        prediction_error: best.prediction_error,
                    };
                }

                // Regular update for similar content
                if best.similarity >= self.config.similarity_threshold && self.config.prefer_updates {
                    if overlap_ok {
                        self.stats.updates += 1;
                        return GateDecision::Update {
                            target_id: best.memory_id.clone(),
                            similarity: best.similarity,
                            update_type: UpdateType::Merge,
                            prediction_error: best.prediction_error,
                        };
                    }

                    // Close in embedding space but no shared vocabulary: two
                    // notes from the same project about different facts. Keep
                    // them as separate memories instead of concatenating one
                    // onto the other.
                    self.stats.creates += 1;
                    return GateDecision::Create {
                        reason: CreateReason::DifferentDomain,
                        prediction_error: best.prediction_error,
                        related_memory_ids: top_candidates
                            .iter()
                            .map(|s| s.memory_id.clone())
                            .collect(),
                    };
                }
            }
        }

        // Check for merge opportunity (multiple similar memories)
        let merge_candidates: Vec<_> = top_candidates
            .iter()
            .filter(|s| s.similarity >= self.config.similarity_threshold * 0.9)
            .collect();

        if merge_candidates.len() >= 2 {
            let avg_similarity = merge_candidates.iter().map(|s| s.similarity).sum::<f32>()
                / merge_candidates.len() as f32;

            self.stats.merges += 1;
            return GateDecision::Merge {
                memory_ids: merge_candidates.iter().map(|s| s.memory_id.clone()).collect(),
                avg_similarity,
                strategy: MergeStrategy::Combine,
            };
        }

        // Default: create new (high prediction error)
        let best_pe = top_candidates
            .first()
            .map(|s| s.prediction_error)
            .unwrap_or(1.0);

        self.stats.creates += 1;
        GateDecision::Create {
            reason: if candidates.is_empty() {
                CreateReason::NoSimilarMemories
            } else {
                CreateReason::HighPredictionError
            },
            prediction_error: best_pe,
            related_memory_ids: top_candidates.iter().map(|s| s.memory_id.clone()).collect(),
        }
    }

    /// Evaluate with explicit intent
    ///
    /// Use when the user has indicated intent (e.g., "update X" or "this is better than Y")
    pub fn evaluate_with_intent(
        &mut self,
        new_content: &str,
        new_embedding: &[f32],
        candidates: &[CandidateMemory],
        intent: EvaluationIntent,
    ) -> GateDecision {
        match intent {
            EvaluationIntent::ForceCreate => {
                self.stats.creates += 1;
                GateDecision::Create {
                    reason: CreateReason::ExplicitCreate,
                    prediction_error: 1.0,
                    related_memory_ids: vec![],
                }
            }
            EvaluationIntent::ForceUpdate { target_id } => {
                // Find the target candidate
                if let Some(c) = candidates.iter().find(|c| c.id == target_id) {
                    let similarity = cosine_similarity(new_embedding, &c.embedding);
                    self.stats.updates += 1;
                    GateDecision::Update {
                        target_id: target_id.clone(),
                        similarity,
                        update_type: UpdateType::Replace,
                        prediction_error: 1.0 - similarity,
                    }
                } else {
                    // Target not found, evaluate normally
                    self.evaluate(new_content, new_embedding, candidates)
                }
            }
            EvaluationIntent::Supersede { old_memory_id, reason } => {
                if let Some(c) = candidates.iter().find(|c| c.id == old_memory_id) {
                    let similarity = cosine_similarity(new_embedding, &c.embedding);
                    self.stats.supersedes += 1;
                    GateDecision::Supersede {
                        old_memory_id,
                        similarity,
                        supersede_reason: reason,
                        prediction_error: 1.0 - similarity,
                    }
                } else {
                    self.evaluate(new_content, new_embedding, candidates)
                }
            }
            EvaluationIntent::Auto => {
                self.evaluate(new_content, new_embedding, candidates)
            }
        }
    }

    /// Detect if two pieces of content appear contradictory.
    ///
    /// Contradiction is a property of the **pair**, and it needs two things:
    /// opposing polarity, and a shared subject for that polarity to disagree
    /// about. Two texts contradict when either
    ///
    /// 1. exactly one of them negates, and they talk about the same things, or
    /// 2. they carry opposite halves of an explicit antonym pair.
    ///
    /// Before `openclaw-vestige-1de` this looked only at the new text, so any
    /// note containing "not ", "fixed" or "improved" read as a contradiction of
    /// whatever happened to sit nearest in embedding space.
    ///
    /// Still a heuristic; an NLI model would do better.
    fn detect_contradiction(&self, new_content: &str, old_content: &str) -> bool {
        let new_lower = new_content.to_lowercase();
        let old_lower = old_content.to_lowercase();

        let new_negates = NEGATION_MARKERS.iter().any(|m| new_lower.contains(m));
        let old_negates = NEGATION_MARKERS.iter().any(|m| old_lower.contains(m));
        let polarity_flip = new_negates != old_negates;
        let same_subject =
            lexical_overlap(&new_lower, &old_lower) >= self.config.min_content_overlap;

        if polarity_flip && same_subject {
            return true;
        }

        ANTONYM_PAIRS.iter().any(|(neg, pos)| {
            (contains_word(&new_lower, neg) && contains_word(&old_lower, pos))
                || (contains_word(&old_lower, neg) && contains_word(&new_lower, pos))
        })
    }

    /// Does the new content explicitly announce itself as an edit?
    ///
    /// This is *intent*, not evidence: "fixed" says the author fixed
    /// something, never which memory it replaces. Callers must pair it with a
    /// lexical-overlap check before acting destructively.
    fn has_correction_marker(new_content: &str) -> bool {
        let new_lower = new_content.to_lowercase();
        CORRECTION_MARKERS
            .iter()
            .any(|phrase| new_lower.contains(phrase))
    }

    /// Get statistics
    pub fn stats(&self) -> &GateStats {
        &self.stats
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = GateStats::default();
    }
}

// ============================================================================
// EVALUATION INTENT
// ============================================================================

/// Explicit intent for evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvaluationIntent {
    /// Automatically determine best action
    Auto,
    /// Force creation of new memory
    ForceCreate,
    /// Force update of specific memory
    ForceUpdate { target_id: String },
    /// Force supersede of specific memory
    Supersede { old_memory_id: String, reason: SupersedeReason },
}

// ============================================================================
// STATISTICS
// ============================================================================

/// Statistics about gate decisions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GateStats {
    /// Total evaluations performed
    pub total_evaluations: usize,
    /// Decisions to create new
    pub creates: usize,
    /// Decisions to update existing
    pub updates: usize,
    /// Decisions to supersede
    pub supersedes: usize,
    /// Decisions to merge
    pub merges: usize,
}

impl GateStats {
    /// Get create rate
    pub fn create_rate(&self) -> f64 {
        if self.total_evaluations > 0 {
            self.creates as f64 / self.total_evaluations as f64
        } else {
            0.0
        }
    }

    /// Get update rate
    pub fn update_rate(&self) -> f64 {
        if self.total_evaluations > 0 {
            self.updates as f64 / self.total_evaluations as f64
        } else {
            0.0
        }
    }

    /// Get supersede rate
    pub fn supersede_rate(&self) -> f64 {
        if self.total_evaluations > 0 {
            self.supersedes as f64 / self.total_evaluations as f64
        } else {
            0.0
        }
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Does `haystack` contain `needle` as a whole word?
///
/// Substring matching would let "correct" match inside "incorrect" and "bad"
/// inside "badge", which inverts the polarity test it feeds.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = haystack[from..].find(needle) {
        let start = from + rel;
        let end = start + needle.len();
        let left_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        // Apostrophes are part of the word ("don't"), so a trailing "'" still
        // counts as a boundary only when the needle already consumed it.
        let right_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if left_ok && right_ok {
            return true;
        }
        from = start + 1;
        if from >= haystack.len() {
            break;
        }
    }
    false
}

/// Content words of a text: lowercase alphanumeric tokens of at least
/// [`MIN_OVERLAP_TOKEN_LEN`] characters that are not common fillers.
fn content_tokens(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= MIN_OVERLAP_TOKEN_LEN)
        .filter(|t| !OVERLAP_STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

/// Jaccard overlap of the content words of two texts, in `0.0..=1.0`.
///
/// Two notes that genuinely correct each other restate the same nouns; two
/// notes that merely come from the same project do not. This is the signal
/// embedding similarity cannot supply on its own.
pub fn lexical_overlap(a: &str, b: &str) -> f32 {
    let ta = content_tokens(a);
    let tb = content_tokens(b);
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let intersection = ta.intersection(&tb).count() as f32;
    let union = ta.union(&tb).count() as f32;
    if union == 0.0 { 0.0 } else { intersection / union }
}

/// Calculate cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    (dot / (norm_a * norm_b)).clamp(0.0, 1.0)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_embedding(seed: f32) -> Vec<f32> {
        // Create embeddings with controlled similarity based on seed
        // Seeds close to each other = similar vectors
        // Seeds far apart = different vectors
        (0..384).map(|i| {
            let base = (i as f32 / 384.0) * std::f32::consts::PI * 2.0;
            (base * seed).sin()
        }).collect()
    }

    fn make_orthogonal_embedding() -> Vec<f32> {
        // Create an embedding that's orthogonal to seed=1.0
        (0..384).map(|i| {
            let base = (i as f32 / 384.0) * std::f32::consts::PI * 2.0;
            (base + std::f32::consts::PI / 2.0).sin()  // 90 degree phase shift
        }).collect()
    }

    fn make_candidate(id: &str, seed: f32) -> CandidateMemory {
        CandidateMemory {
            id: id.to_string(),
            content: format!("Content for {}", id),
            embedding: make_embedding(seed),
            retrieval_strength: 0.8,
            retention_strength: 0.7,
            tags: vec![],
            source: None,
            was_demoted: false,
            was_promoted: false,
        }
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 0.001);

        let d = vec![-1.0, 0.0, 0.0];
        assert!(cosine_similarity(&a, &d) <= 0.0);
    }

    #[test]
    fn test_empty_candidates() {
        let mut gate = PredictionErrorGate::new();
        let embedding = make_embedding(1.0);

        let decision = gate.evaluate("New content", &embedding, &[]);

        assert!(matches!(decision, GateDecision::Create { reason: CreateReason::FirstMemory, .. }));
    }

    #[test]
    fn test_high_similarity_update() {
        let mut gate = PredictionErrorGate::new();
        let embedding = make_embedding(1.0);

        // Create candidate with identical embedding
        let mut candidate = make_candidate("mem-1", 1.0);
        candidate.embedding = embedding.clone();

        let decision = gate.evaluate("Same content", &embedding, &[candidate]);

        assert!(decision.is_update());
        if let GateDecision::Update { update_type, .. } = decision {
            assert_eq!(update_type, UpdateType::Reinforce);
        }
    }

    #[test]
    fn test_demoted_memory_supersede() {
        let mut gate = PredictionErrorGate::new();
        let embedding = make_embedding(1.0);

        // Use similar embedding (seed 1.05) - close enough to be above similarity threshold
        let mut candidate = make_candidate("mem-1", 1.0);
        candidate.embedding = make_embedding(1.05);
        candidate.was_demoted = true;
        // Since openclaw-vestige-1de the supersede branches also require the
        // two texts to share vocabulary, so the candidate content has to be
        // about the same thing as the new content.
        candidate.content =
            "Retry the upload with exponential backoff starting at 200ms".to_string();

        let decision = gate.evaluate(
            "Better solution: retry the upload with exponential backoff starting at 500ms",
            &embedding,
            &[candidate],
        );

        // Should supersede the demoted memory if similarity is above threshold
        // If not superseding, it should at least update
        assert!(matches!(decision, GateDecision::Supersede { .. } | GateDecision::Update { .. }));
    }

    #[test]
    fn test_different_content_create() {
        let mut gate = PredictionErrorGate::new();
        let new_embedding = make_embedding(1.0);

        // Use orthogonal embedding for truly different content
        let mut candidate = make_candidate("mem-1", 1.0);
        candidate.embedding = make_orthogonal_embedding();

        let decision = gate.evaluate("Completely different topic", &new_embedding, &[candidate]);

        assert!(matches!(decision, GateDecision::Create { .. }));
    }

    #[test]
    fn test_contradiction_detection() {
        let gate = PredictionErrorGate::new();

        // One side negates, and they are about the same subject.
        assert!(gate.detect_contradiction(
            "Don't use synchronous code in the request handler",
            "Use synchronous code in the request handler for simplicity"
        ));

        // Symmetric — order of the arguments must not matter.
        assert!(gate.detect_contradiction(
            "Use synchronous code in the request handler for simplicity",
            "Don't use synchronous code in the request handler"
        ));

        // Explicit antonyms need no negation word.
        assert!(gate.detect_contradiction(
            "The batch endpoint is deprecated",
            "The batch endpoint is the recommended path"
        ));

        assert!(!gate.detect_contradiction(
            "Use async/await for performance",
            "Use async patterns when needed"
        ));
    }

    /// A negation only contradicts something it is *about*. Same negation, two
    /// unrelated subjects, must not register.
    #[test]
    fn test_negation_needs_a_shared_subject() {
        let gate = PredictionErrorGate::new();

        assert!(!gate.detect_contradiction(
            "Don't use synchronous code in the request handler",
            "Prefer a single writer per shard so back-pressure stays observable"
        ));
    }

    /// Regression for openclaw-vestige-1de: a correction *marker* in the new
    /// text is intent, not evidence. "Actually..." says nothing about which
    /// memory it corrects, so it is no longer a contradiction on its own.
    #[test]
    fn test_correction_marker_alone_is_not_a_contradiction() {
        let gate = PredictionErrorGate::new();

        assert!(!gate.detect_contradiction(
            "Actually, the approach is to batch the writes",
            "The approach is to stream the writes"
        ));
        assert!(PredictionErrorGate::has_correction_marker(
            "Actually, the approach is to batch the writes"
        ));
    }

    /// Regression for openclaw-vestige-1de: pre-fix, ANY new text containing
    /// "not " counted as contradicting whatever was nearest in embedding space.
    #[test]
    fn test_negation_in_new_text_alone_is_not_a_contradiction() {
        let gate = PredictionErrorGate::new();

        assert!(!gate.detect_contradiction(
            "The dream cycle replays recent memories; it does not run GC",
            "Consolidation runs FSRS decay, then embedding refresh, then GC"
        ));
    }

    #[test]
    fn test_contains_word_respects_boundaries() {
        assert!(contains_word("this is incorrect", "incorrect"));
        // "correct" must not match inside "incorrect"
        assert!(!contains_word("this is incorrect", "correct"));
        assert!(!contains_word("the badger ran", "bad"));
        assert!(contains_word("that is bad", "bad"));
        assert!(contains_word("don't do that", "don't"));
    }

    #[test]
    fn test_lexical_overlap_separates_shared_subject_from_shared_project() {
        // Same subject, restated: high overlap.
        let a = "the ring buffer sequence counter wraps and the harness drops frames";
        let b = "widen the ring buffer sequence counter so the harness stops dropping frames";
        assert!(
            lexical_overlap(a, b) >= MIN_CONTENT_OVERLAP,
            "restated subject should overlap, got {}",
            lexical_overlap(a, b)
        );

        // Same project, different facts: low overlap.
        let c = "Theseus M3a lessons: widen the ring buffer sequence counter to u64";
        let d = "Theseus M3c lessons: keep the scheduler clock source injectable";
        assert!(
            lexical_overlap(c, d) < MIN_CONTENT_OVERLAP,
            "disjoint facts should not overlap, got {}",
            lexical_overlap(c, d)
        );

        assert_eq!(lexical_overlap("", "anything"), 0.0);
        assert_eq!(lexical_overlap("a b c", "d e f"), 0.0); // all tokens too short
    }

    /// Build two unit vectors with an exact cosine similarity.
    fn pair_with_similarity(target: f32) -> (Vec<f32>, Vec<f32>) {
        let mut a = vec![0.0f32; 384];
        let mut b = vec![0.0f32; 384];
        a[0] = 1.0;
        b[0] = target;
        b[1] = (1.0 - target * target).sqrt();
        (a, b)
    }

    /// The reported defect, verbatim: two lesson notes from one project with
    /// disjoint facts, sitting at 0.78-0.80 cosine similarity. Pre-fix this
    /// returned Supersede, which demotes the older node's FSRS strengths.
    #[test]
    fn test_disjoint_lessons_do_not_supersede() {
        let bugfix_lessons = "Theseus M3a/M3b lessons: the harness dropped frames \
                              when the ring buffer wrapped; fixed by widening the \
                              sequence counter to u64.";
        let design_lessons = "Theseus M3c/M3d design lessons: prefer a single \
                              writer per shard so back-pressure is observable, and \
                              keep the scheduler clock source injectable.";

        for sim in [0.78_f32, 0.80] {
            for (new, old) in [
                (bugfix_lessons, design_lessons),
                (design_lessons, bugfix_lessons),
            ] {
                let (new_emb, old_emb) = pair_with_similarity(sim);
                let mut gate = PredictionErrorGate::new();
                let mut candidate = make_candidate("old-node", 1.0);
                candidate.content = old.to_string();
                candidate.embedding = old_emb;
                candidate.retrieval_strength = 1.0; // healthy, never demoted

                let decision = gate.evaluate(new, &new_emb, &[candidate]);

                assert!(
                    !matches!(decision, GateDecision::Supersede { .. }),
                    "sim={sim} must not supersede disjoint lessons, got {decision:?}"
                );
                match decision {
                    GateDecision::Create { reason, .. } => assert_eq!(
                        reason,
                        CreateReason::DifferentDomain,
                        "sim={sim} should keep the memories separate"
                    ),
                    other => panic!("sim={sim} expected a separate memory, got {other:?}"),
                }
            }
        }
    }

    /// The other half of the contract: a real correction of the same fact must
    /// still supersede, or the fix would have simply disabled the feature.
    #[test]
    fn test_genuine_correction_still_supersedes() {
        let old = "Theseus M3a: the harness drops frames when the ring buffer \
                   wraps. Root cause: the sequence counter is u32 and wraps at 4 \
                   billion frames. Always widen the sequence counter to u64.";
        let new = "Theseus M3a correction: widening the ring buffer sequence \
                   counter to u64 was wrong — the counter never wraps. The harness \
                   drops frames because the reader never acknowledges the wrap.";

        let (new_emb, old_emb) = pair_with_similarity(0.88);
        let mut gate = PredictionErrorGate::new();
        let mut candidate = make_candidate("old-node", 1.0);
        candidate.content = old.to_string();
        candidate.embedding = old_emb;

        let decision = gate.evaluate(new, &new_emb, &[candidate]);
        assert!(
            matches!(decision, GateDecision::Supersede { .. }),
            "genuine correction should supersede, got {decision:?}"
        );
    }

    /// Supersede is destructive, so it must be harder to reach than a plain
    /// update — not easier, which is what a correction threshold below the
    /// similarity threshold produced.
    #[test]
    fn test_correction_threshold_is_above_similarity_threshold() {
        let config = PredictionErrorConfig::default();
        assert!(
            config.correction_threshold > config.similarity_threshold,
            "correction_threshold {} must exceed similarity_threshold {}",
            config.correction_threshold,
            config.similarity_threshold
        );
        assert!(config.correction_threshold < config.near_identical_threshold);
    }

    /// A demoted candidate must also clear the overlap bar, otherwise a node
    /// that once dipped below 0.3 retrieval strength gets demoted again by
    /// every unrelated neighbour — a ratchet toward auto-GC.
    #[test]
    fn test_demoted_candidate_still_needs_overlap() {
        let (new_emb, old_emb) = pair_with_similarity(0.80);
        let mut gate = PredictionErrorGate::new();
        let mut candidate = make_candidate("old-node", 1.0);
        candidate.content =
            "Theseus M3c design lessons: keep the scheduler clock source injectable".to_string();
        candidate.embedding = old_emb;
        candidate.was_demoted = true;

        let decision = gate.evaluate(
            "Theseus M3a lessons: widen the ring buffer sequence counter to u64",
            &new_emb,
            &[candidate],
        );

        assert!(
            !matches!(decision, GateDecision::Supersede { .. }),
            "demoted node with disjoint content must not be superseded, got {decision:?}"
        );
    }

    #[test]
    fn test_force_create_intent() {
        let mut gate = PredictionErrorGate::new();
        let embedding = make_embedding(1.0);
        let candidate = make_candidate("mem-1", 1.0);

        let decision = gate.evaluate_with_intent(
            "New content",
            &embedding,
            &[candidate],
            EvaluationIntent::ForceCreate,
        );

        assert!(matches!(decision, GateDecision::Create { reason: CreateReason::ExplicitCreate, .. }));
    }

    #[test]
    fn test_force_update_intent() {
        let mut gate = PredictionErrorGate::new();
        let embedding = make_embedding(1.0);
        let candidate = make_candidate("mem-1", 5.0);

        let decision = gate.evaluate_with_intent(
            "Updated content",
            &embedding,
            &[candidate],
            EvaluationIntent::ForceUpdate { target_id: "mem-1".to_string() },
        );

        assert!(matches!(decision, GateDecision::Update { .. }));
    }

    #[test]
    fn test_stats() {
        let mut gate = PredictionErrorGate::new();
        let embedding = make_embedding(1.0);

        // Create (empty candidates)
        gate.evaluate("Content", &embedding, &[]);

        // Update (identical)
        let mut candidate = make_candidate("mem-1", 1.0);
        candidate.embedding = embedding.clone();
        gate.evaluate("Content", &embedding, &[candidate.clone()]);

        let stats = gate.stats();
        assert_eq!(stats.total_evaluations, 2);
        assert_eq!(stats.creates, 1);
        assert_eq!(stats.updates, 1);
    }
}
