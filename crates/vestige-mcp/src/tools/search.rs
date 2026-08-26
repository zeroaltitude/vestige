//! Search Tools (Deprecated - use search_unified instead)
//!
//! Semantic and hybrid search implementations.

use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use vestige_core::Storage;

/// Input schema for semantic_search tool
pub fn semantic_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Search query for semantic similarity"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of results (default: 10)",
                "default": 10,
                "minimum": 1,
                "maximum": 50
            },
            "min_similarity": {
                "type": "number",
                "description": "Minimum similarity threshold (0.0-1.0, default: 0.5)",
                "default": 0.5,
                "minimum": 0.0,
                "maximum": 1.0
            }
        },
        "required": ["query"]
    })
}

/// Input schema for hybrid_search tool
pub fn hybrid_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Search query"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of results (default: 10)",
                "default": 10,
                "minimum": 1,
                "maximum": 50
            },
            "keyword_weight": {
                "type": "number",
                "description": "Weight for keyword search (0.0-1.0, default: 0.5)",
                "default": 0.5,
                "minimum": 0.0,
                "maximum": 1.0
            },
            "semantic_weight": {
                "type": "number",
                "description": "Weight for semantic search (0.0-1.0, default: 0.5)",
                "default": 0.5,
                "minimum": 0.0,
                "maximum": 1.0
            }
        },
        "required": ["query"]
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SemanticSearchArgs {
    query: String,
    limit: Option<i32>,
    #[serde(alias = "min_similarity")]
    min_similarity: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HybridSearchArgs {
    query: String,
    limit: Option<i32>,
    #[serde(alias = "keyword_weight")]
    keyword_weight: Option<f32>,
    #[serde(alias = "semantic_weight")]
    semantic_weight: Option<f32>,
}

pub async fn execute_semantic(
    storage: &Arc<Storage>,
    args: Option<Value>,
) -> Result<Value, String> {
    let args: SemanticSearchArgs = match args {
        Some(v) => serde_json::from_value(v).map_err(|e| format!("Invalid arguments: {}", e))?,
        None => return Err("Missing arguments".to_string()),
    };

    if args.query.trim().is_empty() {
        return Err("Query cannot be empty".to_string());
    }


    // Check if embeddings are ready
    if !storage.is_embedding_ready() {
        return Ok(serde_json::json!({
            "error": "Embedding service not ready",
            "hint": "Run consolidation first to initialize embeddings, or the model may still be loading.",
        }));
    }

    let results = storage
        .semantic_search(
            &args.query,
            args.limit.unwrap_or(10).clamp(1, 50),
            args.min_similarity.unwrap_or(0.5).clamp(0.0, 1.0),
        )
        .map_err(|e| e.to_string())?;

    let formatted: Vec<Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.node.id,
                "content": r.node.content,
                "similarity": r.similarity,
                "nodeType": r.node.node_type,
                "tags": r.node.tags,
                "retentionStrength": r.node.retention_strength,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "query": args.query,
        "method": "semantic",
        "total": formatted.len(),
        "results": formatted,
    }))
}

pub async fn execute_hybrid(
    storage: &Arc<Storage>,
    args: Option<Value>,
) -> Result<Value, String> {
    let args: HybridSearchArgs = match args {
        Some(v) => serde_json::from_value(v).map_err(|e| format!("Invalid arguments: {}", e))?,
        None => return Err("Missing arguments".to_string()),
    };

    if args.query.trim().is_empty() {
        return Err("Query cannot be empty".to_string());
    }


    let results = storage
        .hybrid_search(
            &args.query,
            args.limit.unwrap_or(10).clamp(1, 50),
            args.keyword_weight.unwrap_or(0.3).clamp(0.0, 1.0),
            args.semantic_weight.unwrap_or(0.7).clamp(0.0, 1.0),
        )
        .map_err(|e| e.to_string())?;

    let formatted: Vec<Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.node.id,
                "content": r.node.content,
                "combinedScore": r.combined_score,
                "keywordScore": r.keyword_score,
                "semanticScore": r.semantic_score,
                "matchType": format!("{:?}", r.match_type),
                "nodeType": r.node.node_type,
                "tags": r.node.tags,
                "retentionStrength": r.node.retention_strength,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "query": args.query,
        "method": "hybrid",
        "total": formatted.len(),
        "results": formatted,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// semantic_schema()/hybrid_schema() advertise snake_case names, but the arg
    /// structs are camelCase. Both spellings must reach the same fields.
    #[test]
    fn test_args_accept_both_spellings() {
        let snake: SemanticSearchArgs =
            serde_json::from_value(serde_json::json!({"query": "q", "min_similarity": 0.7}))
                .unwrap();
        let camel: SemanticSearchArgs =
            serde_json::from_value(serde_json::json!({"query": "q", "minSimilarity": 0.7}))
                .unwrap();
        assert_eq!(snake.min_similarity, Some(0.7));
        assert_eq!(snake.min_similarity, camel.min_similarity);

        let h_snake: HybridSearchArgs = serde_json::from_value(
            serde_json::json!({"query": "q", "keyword_weight": 0.4, "semantic_weight": 0.6}),
        )
        .unwrap();
        let h_camel: HybridSearchArgs = serde_json::from_value(
            serde_json::json!({"query": "q", "keywordWeight": 0.4, "semanticWeight": 0.6}),
        )
        .unwrap();
        assert_eq!(h_snake.keyword_weight, Some(0.4));
        assert_eq!(h_snake.semantic_weight, Some(0.6));
        assert_eq!(h_snake.keyword_weight, h_camel.keyword_weight);
        assert_eq!(h_snake.semantic_weight, h_camel.semantic_weight);
    }
}
