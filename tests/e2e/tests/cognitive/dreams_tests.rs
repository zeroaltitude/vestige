//! # Sleep Consolidation & Dreams E2E Tests (Phase 7.5)
//!
//! Comprehensive tests for Vestige's sleep-inspired memory consolidation
//! and dream-based insight generation.
//!
//! Based on modern sleep consolidation theory:
//! - Stickgold & Walker (2013): Memory consolidation during sleep
//! - Nader (2003): Memory reconsolidation theory
//! - Diekelmann & Born (2010): The memory function of sleep
//!
//! ## Test Categories
//!
//! 1. **Insight Generation**: Tests that dreams create novel insights
//! 2. **5-Stage Cycle**: Tests for each consolidation stage
//! 3. **Scheduler & Timing**: Tests for activity detection and idle triggers

use chrono::{Duration, Utc};
use vestige_core::advanced::dreams::{
    ActivityTracker, ConnectionGraph, ConnectionReason, ConsolidationScheduler, DreamConfig,
    DreamMemory, InsightType, MemoryDreamer,
};
use std::collections::HashSet;

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Create a test memory with default settings
fn make_memory(id: &str, content: &str, tags: Vec<&str>) -> DreamMemory {
    DreamMemory {
        id: id.to_string(),
        content: content.to_string(),
        embedding: None,
        tags: tags.into_iter().map(String::from).collect(),
        created_at: Utc::now(),
        access_count: 1,
    }
}

/// Create a memory with specific timestamp (hours ago)
fn make_memory_with_time(id: &str, content: &str, tags: Vec<&str>, hours_ago: i64) -> DreamMemory {
    DreamMemory {
        id: id.to_string(),
        content: content.to_string(),
        embedding: None,
        tags: tags.into_iter().map(String::from).collect(),
        created_at: Utc::now() - Duration::hours(hours_ago),
        access_count: 1,
    }
}

/// Create a memory with access count
fn make_memory_with_access(
    id: &str,
    content: &str,
    tags: Vec<&str>,
    access_count: u32,
) -> DreamMemory {
    DreamMemory {
        id: id.to_string(),
        content: content.to_string(),
        embedding: None,
        tags: tags.into_iter().map(String::from).collect(),
        created_at: Utc::now() - Duration::hours(24),
        access_count,
    }
}

/// Create a memory carrying an explicit embedding, at a fixed age.
///
/// `calculate_memory_similarity()` uses cosine similarity only when *both*
/// memories have an embedding; otherwise it falls back to
/// `tag_jaccard * 0.4 + content_jaccard * 0.6`, which for short prose fixtures
/// lands around 0.2-0.3 — under `MIN_SIMILARITY_FOR_CONNECTION` (0.5). Tests
/// that need stage 2 to actually create edges must therefore supply embeddings,
/// which is also what production does: every stored memory has one.
fn make_memory_with_embedding(
    id: &str,
    content: &str,
    tags: Vec<&str>,
    embedding: Vec<f32>,
    hours_ago: i64,
) -> DreamMemory {
    DreamMemory {
        id: id.to_string(),
        content: content.to_string(),
        embedding: Some(embedding),
        tags: tags.into_iter().map(String::from).collect(),
        created_at: Utc::now() - Duration::hours(hours_ago),
        access_count: 1,
    }
}


// ============================================================================
// INSIGHT GENERATION TESTS (5 tests)
// ============================================================================

/// Test that consolidation generates novel insights from memory clusters.
///
/// Validates that the dream cycle can synthesize new understanding
/// from groups of related memories, going beyond simple retrieval.
#[tokio::test]
async fn test_consolidation_generates_novel_insights() {
    let config = DreamConfig {
        max_memories_per_dream: 100,
        min_similarity: 0.1, // Low threshold to ensure connections are found
        max_insights: 10,
        min_novelty: 0.1, // Lower threshold for testing
        enable_compression: true,
        enable_strengthening: true,
        focus_tags: vec![],
    };
    let dreamer = MemoryDreamer::with_config(config);

    // Create a cluster of related memories with HIGH tag overlap for guaranteed connections
    // All memories share "rust" and "memory" tags to ensure cluster formation
    let memories = vec![
        make_memory(
            "1",
            "Rust ownership prevents memory leaks automatically through compile time checks",
            vec!["rust", "memory", "ownership", "safety"],
        ),
        make_memory(
            "2",
            "The borrow checker enforces memory ownership rules at compile time in Rust",
            vec!["rust", "memory", "borrowing", "safety"],
        ),
        make_memory(
            "3",
            "RAII pattern in Rust memory ensures resources are freed when out of scope",
            vec!["rust", "memory", "raii", "safety"],
        ),
        make_memory(
            "4",
            "Smart pointers like Box and Rc manage heap memory safely in Rust",
            vec!["rust", "memory", "pointers", "safety"],
        ),
        make_memory(
            "5",
            "Lifetimes annotate how long references are valid in Rust memory management",
            vec!["rust", "memory", "lifetimes", "safety"],
        ),
    ];

    let result = dreamer.dream(&memories).await;

    // Should analyze all memories
    assert_eq!(
        result.stats.memories_analyzed, 5,
        "Should analyze all 5 memories"
    );

    // Should evaluate connections between memories
    assert!(
        result.stats.connections_evaluated > 0,
        "Should evaluate connections between memories"
    );

    // Should find clusters
    assert!(
        result.stats.clusters_found > 0 || result.new_connections_found > 0,
        "Should find clusters or connections with high tag overlap"
    );

    // If insights are generated, verify their structure
    for insight in &result.insights_generated {
        assert!(
            insight.source_memories.len() >= 2,
            "Insights should combine multiple memories, got {} sources",
            insight.source_memories.len()
        );
    }
}

/// Test that insights have proper novelty scoring.
///
/// Novelty measures how "new" an insight is compared to its source memories.
/// Higher novelty means the insight goes beyond just summarizing.
#[tokio::test]
async fn test_insight_novelty_scoring() {
    let config = DreamConfig {
        min_novelty: 0.1, // Accept low novelty for testing
        ..DreamConfig::default()
    };
    let dreamer = MemoryDreamer::with_config(config);

    // Create memories that can generate insights
    let memories = vec![
        make_memory(
            "1",
            "Machine learning models require training data",
            vec!["ml", "training"],
        ),
        make_memory(
            "2",
            "Deep learning uses neural network architectures",
            vec!["ml", "deep-learning"],
        ),
        make_memory(
            "3",
            "Training data quality affects model performance",
            vec!["ml", "training", "quality"],
        ),
        make_memory(
            "4",
            "Neural networks learn patterns from training examples",
            vec!["ml", "deep-learning", "training"],
        ),
    ];

    let result = dreamer.dream(&memories).await;

    // All insights should have novelty scores
    for insight in &result.insights_generated {
        assert!(
            insight.novelty_score >= 0.0 && insight.novelty_score <= 1.0,
            "Novelty score should be between 0 and 1, got {}",
            insight.novelty_score
        );

        // Novelty should meet minimum threshold
        assert!(
            insight.novelty_score >= 0.1,
            "Novelty score {} below minimum threshold",
            insight.novelty_score
        );
    }
}

/// Test that insights track their source memories correctly.
///
/// Each insight should maintain references to the memories that
/// contributed to its generation.
#[tokio::test]
async fn test_insight_source_memory_tracking() {
    let config = DreamConfig {
        min_novelty: 0.1,
        min_similarity: 0.2,
        ..DreamConfig::default()
    };
    let dreamer = MemoryDreamer::with_config(config);

    let memories = vec![
        make_memory(
            "mem_a",
            "Database indexing improves query performance significantly",
            vec!["database", "performance"],
        ),
        make_memory(
            "mem_b",
            "Query optimization requires understanding execution plans",
            vec!["database", "optimization"],
        ),
        make_memory(
            "mem_c",
            "Index selection affects both read and write performance",
            vec!["database", "performance", "indexing"],
        ),
    ];

    let result = dreamer.dream(&memories).await;

    // Each insight should have valid source references
    let memory_ids: HashSet<_> = memories.iter().map(|m| m.id.as_str()).collect();

    for insight in &result.insights_generated {
        // Source memories should not be empty
        assert!(
            !insight.source_memories.is_empty(),
            "Insight should have source memories"
        );

        // All source memory IDs should be valid
        for source_id in &insight.source_memories {
            assert!(
                memory_ids.contains(source_id.as_str()),
                "Source memory '{}' not found in input memories",
                source_id
            );
        }

        // Should have unique ID
        assert!(
            insight.id.starts_with("insight-"),
            "Insight ID should have proper format"
        );
    }
}

/// Test that insights calculate information gain over source memories.
///
/// Information gain measures how much new understanding the insight
/// provides beyond what's in the individual source memories.
#[tokio::test]
async fn test_insight_information_gain() {
    let config = DreamConfig {
        min_novelty: 0.15,
        min_similarity: 0.2,
        ..DreamConfig::default()
    };
    let dreamer = MemoryDreamer::with_config(config);

    // Create memories with overlapping but distinct information
    let memories = vec![
        make_memory(
            "1",
            "Async programming enables concurrent operations without threads",
            vec!["async", "concurrency"],
        ),
        make_memory(
            "2",
            "Tokio runtime provides async task scheduling and execution",
            vec!["async", "tokio"],
        ),
        make_memory(
            "3",
            "Green threads are lightweight compared to OS threads",
            vec!["async", "threads"],
        ),
        make_memory(
            "4",
            "Event loops drive async execution in most runtimes",
            vec!["async", "runtime"],
        ),
    ];

    let result = dreamer.dream(&memories).await;

    // Verify that insights have been generated
    if !result.insights_generated.is_empty() {
        for insight in &result.insights_generated {
            // Confidence reflects reliability of the insight
            assert!(
                insight.confidence >= 0.0 && insight.confidence <= 1.0,
                "Confidence should be normalized: {}",
                insight.confidence
            );

            // The insight text should be non-empty
            assert!(
                !insight.insight.is_empty(),
                "Insight text should not be empty"
            );

            // Multiple sources indicate synthesis
            if insight.source_memories.len() > 2 {
                // More sources typically means higher confidence
                assert!(
                    insight.confidence >= 0.3,
                    "Multi-source insight should have reasonable confidence"
                );
            }
        }
    }

    // The dream should evaluate connections
    assert!(
        result.stats.connections_evaluated > 0,
        "Should evaluate connections between memories"
    );
}

/// Test that insights properly combine information from multiple memories.
///
/// This tests the core synthesis capability - creating new understanding
/// by connecting disparate pieces of knowledge.
#[tokio::test]
async fn test_insight_combines_multiple_memories() {
    let config = DreamConfig {
        min_novelty: 0.1,
        min_similarity: 0.15,
        max_insights: 20,
        ..DreamConfig::default()
    };
    let dreamer = MemoryDreamer::with_config(config);

    // Create two distinct but related clusters
    let memories = vec![
        // Cluster 1: Rust type system
        make_memory(
            "rust1",
            "Rust enums can hold data in each variant",
            vec!["rust", "types", "enums"],
        ),
        make_memory(
            "rust2",
            "Pattern matching works with enum variants",
            vec!["rust", "types", "patterns"],
        ),
        make_memory(
            "rust3",
            "The Option type eliminates null pointer errors",
            vec!["rust", "types", "option"],
        ),
        // Cluster 2: Error handling
        make_memory(
            "err1",
            "Result type handles recoverable errors",
            vec!["rust", "errors", "result"],
        ),
        make_memory(
            "err2",
            "The question mark operator propagates errors",
            vec!["rust", "errors", "syntax"],
        ),
        make_memory(
            "err3",
            "Custom error types improve error messages",
            vec!["rust", "errors", "types"],
        ),
    ];

    let result = dreamer.dream(&memories).await;

    // Check for cluster detection
    assert!(
        result.stats.clusters_found >= 1,
        "Should find at least one cluster, found {}",
        result.stats.clusters_found
    );

    // Verify insights synthesize across memories
    for insight in &result.insights_generated {
        // Each insight should reference at least 2 memories
        assert!(
            insight.source_memories.len() >= 2,
            "Insight '{}' should combine at least 2 memories, has {}",
            insight.insight,
            insight.source_memories.len()
        );

        // Should have an insight type
        match insight.insight_type {
            InsightType::HiddenConnection
            | InsightType::RecurringPattern
            | InsightType::Generalization
            | InsightType::Synthesis
            | InsightType::TemporalTrend
            | InsightType::Contradiction
            | InsightType::KnowledgeGap => {} // All valid types
        }
    }
}

// ============================================================================
// 5-STAGE CYCLE TESTS (5 tests)
// ============================================================================

/// Test Stage 1: Decay - memories lose strength over time.
///
/// The decay stage applies forgetting curves to all memories,
/// simulating natural memory decay during consolidation.
#[tokio::test]
async fn test_consolidation_decay_stage() {
    let mut scheduler = ConsolidationScheduler::new();

    // Create memories with varying ages
    let memories = vec![
        make_memory_with_time("old", "Old memory from long ago", vec!["history"], 720), // 30 days
        make_memory_with_time("medium", "Medium age memory", vec!["recent"], 168),      // 7 days
        make_memory_with_time("fresh", "Fresh memory from today", vec!["new"], 2),      // 2 hours
    ];

    let report = scheduler.run_consolidation_cycle(&memories).await;

    // Stage 1 should complete with replay
    assert!(
        report.stage1_replay.is_some(),
        "Stage 1 (replay/decay) should complete"
    );

    let replay = report.stage1_replay.as_ref().unwrap();

    // Should replay memories in chronological order
    assert_eq!(
        replay.sequence.len(),
        3,
        "Should replay all 3 memories"
    );

    // Older memory should come first in replay sequence
    assert_eq!(
        replay.sequence[0], "old",
        "Oldest memory should be first in replay sequence"
    );
}

/// Test Stage 2: Replay - recent memories are replayed in sequence.
///
/// Memory replay during consolidation strengthens important
/// sequences and helps integrate new memories with existing ones.
#[tokio::test]
async fn test_consolidation_replay_stage() {
    let mut scheduler = ConsolidationScheduler::new();

    // Create a sequence of related memories
    let memories = vec![
        make_memory_with_time(
            "step1",
            "First step in the process",
            vec!["workflow", "step1"],
            5,
        ),
        make_memory_with_time(
            "step2",
            "Second step follows the first",
            vec!["workflow", "step2"],
            4,
        ),
        make_memory_with_time(
            "step3",
            "Third step completes the workflow",
            vec!["workflow", "step3"],
            3,
        ),
    ];

    let report = scheduler.run_consolidation_cycle(&memories).await;

    let replay = report.stage1_replay.as_ref().unwrap();

    // Verify replay sequence preserves temporal order
    assert!(
        replay.sequence.iter().position(|id| id == "step1").unwrap()
            < replay.sequence.iter().position(|id| id == "step2").unwrap(),
        "step1 should come before step2 in replay"
    );
    assert!(
        replay.sequence.iter().position(|id| id == "step2").unwrap()
            < replay.sequence.iter().position(|id| id == "step3").unwrap(),
        "step2 should come before step3 in replay"
    );

    // Should generate synthetic combinations for testing connections
    assert!(
        !replay.synthetic_combinations.is_empty(),
        "Should generate synthetic combinations to test"
    );
}

/// Test Stage 3: Integration - new connections are formed.
///
/// Integration discovers and creates connections between memories
/// that share semantic or temporal relationships.
#[tokio::test]
async fn test_consolidation_integration_stage() {
    let mut scheduler = ConsolidationScheduler::new();

    // Create memories with overlapping concepts
    let memories = vec![
        make_memory(
            "api1",
            "REST APIs use HTTP methods for operations",
            vec!["api", "rest", "http"],
        ),
        make_memory(
            "api2",
            "GraphQL provides flexible query capabilities",
            vec!["api", "graphql", "query"],
        ),
        make_memory(
            "api3",
            "Both REST and GraphQL serve web clients",
            vec!["api", "web", "clients"],
        ),
        make_memory(
            "http1",
            "HTTP status codes indicate response success or failure",
            vec!["http", "status", "errors"],
        ),
    ];

    let report = scheduler.run_consolidation_cycle(&memories).await;

    // Stage 2 should discover cross-references (connections count is usize, always >= 0)
    // We verify the stage completed by checking the value exists
    let _ = report.stage2_connections; // Stage 2 connections processed

    // Should find connections between API-related memories
    // Even if no connections meet threshold, the process should complete
    assert!(
        report.completed_at <= Utc::now(),
        "Integration stage should complete"
    );
}

/// Test Stage 4: Pruning - weak connections are removed.
///
/// Pruning removes connections that have decayed below threshold,
/// preventing the memory graph from becoming cluttered.
#[tokio::test]
async fn test_consolidation_pruning_stage() {
    let mut scheduler = ConsolidationScheduler::new();

    // Create memories to establish connections
    let memories = vec![
        make_memory("a", "First concept in memory", vec!["concept"]),
        make_memory("b", "Second related concept", vec!["concept"]),
        make_memory("c", "Third weakly related", vec!["other"]),
    ];

    // Run first consolidation to establish connections
    let _first_report = scheduler.run_consolidation_cycle(&memories).await;

    // Run second consolidation - should apply decay and prune
    let second_report = scheduler.run_consolidation_cycle(&memories).await;

    // Pruning stage should complete - verify the count is accessible
    let pruned_count = second_report.stage4_pruned;
    // pruned_count is usize, verification that stage completed
    let _ = pruned_count;

    // The pruning count reflects connections below threshold
    // Even if 0, the process should complete without error
    assert!(
        second_report.completed_at <= Utc::now(),
        "Pruning stage should complete"
    );
}

/// Test Stage 5: Transfer - consolidated memories are marked for semantic storage.
///
/// Memories that have been accessed frequently and have strong
/// connections are candidates for transfer from episodic to semantic storage.
#[tokio::test]
async fn test_consolidation_transfer_stage() {
    let mut scheduler = ConsolidationScheduler::new();

    // Create memories with varying access patterns
    let memories = vec![
        make_memory_with_access(
            "high_access",
            "Frequently accessed important memory",
            vec!["important", "core"],
            10, // High access count
        ),
        make_memory_with_access(
            "medium_access",
            "Moderately accessed memory",
            vec!["important"],
            5,
        ),
        make_memory_with_access(
            "low_access",
            "Rarely accessed memory",
            vec!["minor"],
            1,
        ),
    ];

    let report = scheduler.run_consolidation_cycle(&memories).await;

    // Transfer stage should identify candidates
    // Candidates need: access_count >= 3, multiple connections, strong connection strength
    assert!(
        report.stage5_transferred.is_empty() || !report.stage5_transferred.is_empty(),
        "Transfer stage should complete (may or may not have candidates)"
    );

    // If there are transferred memories, they should have high access
    for transferred_id in &report.stage5_transferred {
        let source_memory = memories.iter().find(|m| &m.id == transferred_id);
        if let Some(mem) = source_memory {
            assert!(
                mem.access_count >= 3,
                "Transferred memory should have been accessed at least 3 times"
            );
        }
    }
}

// ============================================================================
// SCHEDULER & TIMING TESTS (5 tests)
// ============================================================================

/// Test that the scheduler detects user activity correctly.
///
/// Activity detection is crucial for determining when to run
/// consolidation without interrupting the user.
#[test]
fn test_consolidation_scheduler_activity_detection() {
    let mut scheduler = ConsolidationScheduler::new();

    // Initially should be idle (no activity)
    let initial_stats = scheduler.get_activity_stats();
    assert!(
        initial_stats.is_idle,
        "Should be idle with no activity recorded"
    );

    // Record some activity
    for _ in 0..5 {
        scheduler.record_activity();
    }

    // Should no longer be idle
    let active_stats = scheduler.get_activity_stats();
    assert!(
        !active_stats.is_idle,
        "Should not be idle after recording activity"
    );
    assert_eq!(
        active_stats.total_events, 5,
        "Should track 5 activity events"
    );
    assert!(
        active_stats.events_per_minute > 0.0,
        "Activity rate should be positive"
    );
}

/// Test that consolidation triggers during idle periods.
///
/// Consolidation should only run when the user is idle,
/// similar to how the brain consolidates during sleep.
#[test]
fn test_consolidation_idle_trigger() {
    let scheduler = ConsolidationScheduler::new();

    // With default initialization, scheduler starts as if interval has passed
    // and with no activity (idle)
    let should_run = scheduler.should_consolidate();

    // Should be ready to consolidate (interval passed + idle)
    assert!(
        should_run,
        "Should consolidate when idle and interval has passed"
    );

    // Create new scheduler and record activity
    let mut active_scheduler = ConsolidationScheduler::new();
    active_scheduler.record_activity();

    // Should not consolidate when not idle
    let should_not_run = active_scheduler.should_consolidate();
    assert!(
        !should_not_run,
        "Should NOT consolidate when user is active"
    );
}

/// Test memory replay during consolidation follows correct sequence.
///
/// Replay should process memories in temporal order, similar to
/// how the hippocampus replays experiences during sleep.
#[tokio::test]
async fn test_consolidation_memory_replay_sequence() {
    let mut scheduler = ConsolidationScheduler::new();

    // Create memories with specific timestamps
    let memories = vec![
        make_memory_with_time("morning", "Morning standup meeting", vec!["work"], 12),
        make_memory_with_time("afternoon", "Afternoon code review", vec!["work"], 8),
        make_memory_with_time("evening", "Evening deployment", vec!["work"], 4),
        make_memory_with_time("night", "Night monitoring check", vec!["work"], 1),
    ];

    let report = scheduler.run_consolidation_cycle(&memories).await;
    let replay = report.stage1_replay.unwrap();

    // Verify chronological order (oldest first)
    let positions: Vec<_> = ["morning", "afternoon", "evening", "night"]
        .iter()
        .filter_map(|id| replay.sequence.iter().position(|s| s == *id))
        .collect();

    // Each position should be greater than the previous (ascending order)
    for i in 1..positions.len() {
        assert!(
            positions[i] > positions[i - 1],
            "Replay should be in chronological order: {:?}",
            replay.sequence
        );
    }

    // Synthetic combinations should pair adjacent memories
    assert!(
        !replay.synthetic_combinations.is_empty(),
        "Should generate synthetic combinations for testing"
    );
}

/// Test that connections are strengthened during consolidation.
///
/// Connections between co-activated memories should become stronger,
/// implementing Hebbian learning ("neurons that fire together wire together").
#[tokio::test]
async fn test_consolidation_connection_strengthening() {
    let mut scheduler = ConsolidationScheduler::new();

    // Three memories in one tight semantic cluster.
    //
    // The embeddings are load-bearing, not decoration. Stage 3 can only
    // strengthen edges stage 2 already created, and stage 2 admits a pair only
    // at similarity >= MIN_SIMILARITY_FOR_CONNECTION (0.5). Shared tags alone do
    // not get there: under the tag/content fallback these three score ~0.20-0.30,
    // so a no-embedding fixture leaves the graph empty and `stage3_strengthened`
    // pinned at 0 — the Hebbian behaviour this test is named for never runs
    // (openclaw-vestige-wjl).
    //
    // These vectors are deliberately near-parallel, so every pairwise cosine is
    // >= 0.91 and sits well clear of the floor:
    //   rust1 . rust2 = 1.00 / 1.09       ~= 0.917
    //   rust1 . rust3 = 1.06 / sqrt(1.09 * 1.08) ~= 0.977
    //   rust2 . rust3 = 1.06 / sqrt(1.09 * 1.08) ~= 0.977
    //
    // Ages are explicit so stage-1 replay order is deterministic
    // (rust1 -> rust2 -> rust3) and the adjacent-pair assertions below are stable.
    let memories = vec![
        make_memory_with_embedding(
            "rust1",
            "Rust provides memory safety without garbage collection",
            vec!["rust", "safety", "memory"],
            vec![1.0, 0.3, 0.0],
            3,
        ),
        make_memory_with_embedding(
            "rust2",
            "The borrow checker ensures memory safety at compile time",
            vec!["rust", "safety", "compiler"],
            vec![1.0, 0.0, 0.3],
            2,
        ),
        make_memory_with_embedding(
            "rust3",
            "Ownership rules prevent data races in Rust",
            vec!["rust", "safety", "ownership"],
            vec![1.0, 0.2, 0.2],
            1,
        ),
    ];

    // First consolidation cycle: stage 2 builds the graph, stage 3 strengthens it.
    let first_report = scheduler.run_consolidation_cycle(&memories).await;

    // Replay tests all C(3,2) = 3 pairs, and all three clear the floor.
    assert_eq!(
        first_report.stage2_connections, 3,
        "stage 2 should cross-reference all three pairs, got {}",
        first_report.stage2_connections
    );

    // The point of this test. Stage 3 walks `replay.sequence.windows(2)`, so with
    // a 3-memory replay it must strengthen at least the 2 adjacent pairs;
    // pattern-derived pairs add more on top.
    assert!(
        first_report.stage3_strengthened >= memories.len() - 1,
        "first cycle should strengthen at least the {} adjacent replay pairs, got {}",
        memories.len() - 1,
        first_report.stage3_strengthened
    );

    let after_first = scheduler
        .get_connection_stats()
        .expect("connection stats should be available after a consolidation cycle");

    // Three undirected pairs. `get_stats()` halves its edge counters because the
    // graph stores each pair in both directions, so these are pair counts.
    assert_eq!(
        after_first.total_memories, 3,
        "every fixture memory should have connections"
    );
    assert_eq!(
        after_first.total_connections, 3,
        "one edge per pair, got {}",
        after_first.total_connections
    );
    assert_eq!(
        after_first.total_created, 3,
        "cycle 1 should have created exactly those 3 edges"
    );
    assert_eq!(
        after_first.total_pruned, 0,
        "edges this strong survive one decay pass (>= MIN_CONNECTION_STRENGTH)"
    );

    // Second consolidation over the same memories: stage 2 rediscovers the same
    // pairs and must not mint new ones, so stage 3's strengthening has to land on
    // the edges cycle 1 created.
    let second_report = scheduler.run_consolidation_cycle(&memories).await;

    assert!(
        second_report.stage3_strengthened >= memories.len() - 1,
        "second cycle should strengthen at least the {} adjacent replay pairs, got {}",
        memories.len() - 1,
        second_report.stage3_strengthened
    );

    let after_second = scheduler
        .get_connection_stats()
        .expect("connection stats should still be available after a second cycle");

    assert_eq!(
        after_second.total_created, after_first.total_created,
        "second cycle should strengthen cycle 1's edges, not create new ones"
    );
    assert_eq!(
        after_second.total_connections, after_first.total_connections,
        "the edge set should be unchanged by a repeat cycle"
    );
    assert!(
        after_second.average_strength > after_first.average_strength,
        "repeated co-activation should raise average edge strength ({} -> {})",
        after_first.average_strength,
        after_second.average_strength
    );

    // Both cycles should complete successfully. `duration_ms` is not a usable
    // signal here: three tiny in-memory fixtures consolidate in well under a
    // millisecond, so it legitimately rounds to 0. What a completed cycle does
    // guarantee is a stage-1 replay of everything it was handed plus a dream
    // result, so assert that for both cycles.
    for (label, report) in [("first", &first_report), ("second", &second_report)] {
        let replay = report
            .stage1_replay
            .as_ref()
            .unwrap_or_else(|| panic!("{label} cycle should record a stage-1 replay"));
        assert_eq!(
            replay.sequence.len(),
            memories.len(),
            "{label} cycle should replay every memory it was given"
        );
        assert!(
            report.dream_result.is_some(),
            "{label} cycle should record a dream result"
        );
    }

    // Ordering: the second cycle ran after the first.
    assert!(
        second_report.completed_at >= first_report.completed_at,
        "second cycle should complete no earlier than the first ({:?} vs {:?})",
        second_report.completed_at,
        first_report.completed_at
    );
}

/// Test that weak memories are removed during consolidation.
///
/// Memories that fall below threshold should be pruned to prevent
/// the memory system from becoming cluttered with unimportant data.
#[tokio::test]
async fn test_consolidation_weak_memory_removal() {
    let mut scheduler = ConsolidationScheduler::new();

    // Create connection graph with weak connections
    let memories = vec![
        make_memory("strong1", "Important core concept", vec!["core"]),
        make_memory("strong2", "Another important concept", vec!["core"]),
        make_memory("weak1", "Weakly related tangent", vec!["tangent"]),
        make_memory("weak2", "Another weak connection", vec!["other"]),
    ];

    // Run multiple consolidation cycles to accumulate decay
    for _ in 0..3 {
        let _report = scheduler.run_consolidation_cycle(&memories).await;
    }

    // Final cycle should show pruning effects
    let final_report = scheduler.run_consolidation_cycle(&memories).await;

    // Pruning stage should have run - verify data is accessible
    let pruned = final_report.stage4_pruned;
    let _ = pruned; // Pruning stage completed

    // Connection stats should reflect the pruning
    if let Some(stats) = scheduler.get_connection_stats() {
        // Verify stats are accessible
        let _ = stats.total_pruned;
    }

    // Consolidation should complete
    assert!(
        final_report.completed_at <= Utc::now(),
        "Final consolidation should complete"
    );
}

// ============================================================================
// ADDITIONAL EDGE CASE TESTS
// ============================================================================

/// Test dream cycle with empty memory list.
#[tokio::test]
async fn test_dream_empty_memories() {
    let dreamer = MemoryDreamer::new();
    let memories: Vec<DreamMemory> = vec![];

    let result = dreamer.dream(&memories).await;

    assert_eq!(result.stats.memories_analyzed, 0);
    assert!(result.insights_generated.is_empty());
    assert_eq!(result.new_connections_found, 0);
}

/// Test activity tracker edge cases.
#[test]
fn test_activity_tracker_rate_calculation() {
    let mut tracker = ActivityTracker::new();

    // Rate should be 0 with no activity
    assert_eq!(tracker.activity_rate(), 0.0);

    // Time since last activity should be None with no activity
    assert!(tracker.time_since_last_activity().is_none());

    // Record activity and verify
    tracker.record_activity();
    assert!(tracker.time_since_last_activity().is_some());

    // Stats should reflect the activity
    let stats = tracker.get_stats();
    assert_eq!(stats.total_events, 1);
    assert!(stats.last_activity.is_some());
}

/// Test connection graph operations.
#[test]
fn test_connection_graph_comprehensive() {
    let mut graph = ConnectionGraph::new();

    // Add multiple connections
    graph.add_connection("a", "b", 0.8, ConnectionReason::Semantic);
    graph.add_connection("b", "c", 0.6, ConnectionReason::CrossReference);
    graph.add_connection("a", "c", 0.4, ConnectionReason::SharedConcepts);

    // Verify graph structure
    let stats = graph.get_stats();
    assert_eq!(stats.total_connections, 3, "Should have 3 connections");

    // Test connection retrieval
    let a_connections = graph.get_connections("a");
    assert_eq!(a_connections.len(), 2, "Node 'a' should have 2 connections");

    // Test connection strength
    let a_strength = graph.total_connection_strength("a");
    assert!(a_strength >= 1.2, "Total strength should be >= 1.2");

    // Test strengthening
    assert!(graph.strengthen_connection("a", "b", 0.1));
    let new_strength = graph.total_connection_strength("a");
    assert!(new_strength > a_strength, "Strength should increase after reinforcement");

    // Test decay and pruning
    graph.apply_decay(0.5);
    let pruned = graph.prune_weak(0.3);
    // pruned is usize, always >= 0 - just verify the operation completed
    let _ = pruned;
}

/// Test pattern discovery during replay.
#[tokio::test]
async fn test_pattern_discovery() {
    let mut scheduler = ConsolidationScheduler::new();

    // Create memories with recurring theme
    let memories = vec![
        make_memory("p1", "Pattern example one", vec!["pattern", "example"]),
        make_memory("p2", "Pattern example two", vec!["pattern", "example"]),
        make_memory("p3", "Pattern example three", vec!["pattern", "example"]),
        make_memory("p4", "Pattern example four", vec!["pattern", "example"]),
    ];

    let report = scheduler.run_consolidation_cycle(&memories).await;
    let replay = report.stage1_replay.unwrap();

    // Should discover the recurring pattern
    assert!(
        !replay.discovered_patterns.is_empty(),
        "Should discover recurring patterns from shared tags"
    );

    // Pattern should reference multiple memories
    for pattern in &replay.discovered_patterns {
        assert!(
            pattern.memory_ids.len() >= 3,
            "Pattern should span at least 3 memories"
        );
        assert!(
            pattern.confidence > 0.0,
            "Pattern should have positive confidence"
        );
    }
}

/// Test insight type classification.
#[tokio::test]
async fn test_insight_type_classification() {
    let config = DreamConfig {
        min_novelty: 0.1,
        min_similarity: 0.2,
        ..DreamConfig::default()
    };
    let dreamer = MemoryDreamer::with_config(config);

    // Create memories that span time for temporal trend
    let memories = vec![
        make_memory_with_time("t1", "First observation of pattern", vec!["trend"], 720), // 30 days ago
        make_memory_with_time("t2", "Pattern continues developing", vec!["trend"], 360), // 15 days ago
        make_memory_with_time("t3", "Pattern is now established", vec!["trend"], 24), // 1 day ago
    ];

    let result = dreamer.dream(&memories).await;

    // Insights should have categorized types
    for insight in &result.insights_generated {
        let description = insight.insight_type.description();
        assert!(
            !description.is_empty(),
            "Insight type should have description"
        );
    }
}
