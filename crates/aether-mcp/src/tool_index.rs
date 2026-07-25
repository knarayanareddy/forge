//! BM25-lite progressive disclosure index for MCP tools (Ratel borrow).
//!
//! Full ratel-mcp gateway deferred until LOOP-02 — see `docs/RATEL_TOOL_INDEX.md`.

use crate::McpToolInfo;
use std::collections::{HashMap, HashSet};

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

/// In-process BM25 index over MCP tool name + description pairs.
#[derive(Debug, Clone)]
pub struct ToolIndex {
    tools: Vec<McpToolInfo>,
    tokenized: Vec<Vec<String>>,
    doc_freq: HashMap<String, usize>,
    avg_doc_len: f64,
}

impl ToolIndex {
    pub fn from_tools(tools: Vec<McpToolInfo>) -> Self {
        let tokenized: Vec<Vec<String>> = tools
            .iter()
            .map(|t| tokenize(&format!("{} {}", t.name, t.description)))
            .collect();

        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for doc in &tokenized {
            let unique: HashSet<&String> = doc.iter().collect();
            for term in unique {
                *doc_freq.entry(term.clone()).or_default() += 1;
            }
        }

        let avg_doc_len = if tokenized.is_empty() {
            0.0
        } else {
            tokenized.iter().map(|d| d.len() as f64).sum::<f64>() / tokenized.len() as f64
        };

        Self {
            tools,
            tokenized,
            doc_freq,
            avg_doc_len,
        }
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Return top-k `(tool_name, bm25_score)` for `query`, highest score first.
    pub fn search(&self, query: &str, k: usize) -> Vec<(String, f64)> {
        if self.tools.is_empty() || k == 0 {
            return Vec::new();
        }

        let query_terms = tokenize(query);
        if query_terms.is_empty() {
            return Vec::new();
        }

        let n_docs = self.tools.len() as f64;
        let mut scored: Vec<(usize, f64)> = (0..self.tools.len())
            .map(|doc_idx| {
                let doc_len = self.tokenized[doc_idx].len() as f64;
                let mut score = 0.0;

                let mut term_freq: HashMap<&str, usize> = HashMap::new();
                for term in &self.tokenized[doc_idx] {
                    *term_freq.entry(term.as_str()).or_default() += 1;
                }

                for term in &query_terms {
                    let tf = *term_freq.get(term.as_str()).unwrap_or(&0) as f64;
                    if tf == 0.0 {
                        continue;
                    }
                    let df = *self.doc_freq.get(term).unwrap_or(&0) as f64;
                    let idf = ((n_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
                    let norm = tf * (BM25_K1 + 1.0)
                        / (tf + BM25_K1 * (1.0 - BM25_B + BM25_B * doc_len / self.avg_doc_len.max(1.0)));
                    score += idf * norm;
                }

                (doc_idx, score)
            })
            .filter(|(_, s)| *s > 0.0)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored
            .into_iter()
            .take(k)
            .map(|(idx, score)| (self.tools[idx].name.clone(), score))
            .collect()
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tools() -> Vec<McpToolInfo> {
        vec![
            McpToolInfo {
                name: "read_file".into(),
                description: "Read a text file from the granted workspace directory".into(),
                description_hash: "a".into(),
            },
            McpToolInfo {
                name: "write_file".into(),
                description: "Write or overwrite a file in the workspace".into(),
                description_hash: "b".into(),
            },
            McpToolInfo {
                name: "list_directory".into(),
                description: "List files and folders in a directory path".into(),
                description_hash: "c".into(),
            },
            McpToolInfo {
                name: "search_code".into(),
                description: "Search repository source code by regex pattern".into(),
                description_hash: "d".into(),
            },
        ]
    }

    #[test]
    fn tool_index_returns_top_k_by_query_relevance() {
        let index = ToolIndex::from_tools(sample_tools());
        let hits = index.search("read text file workspace", 2);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "read_file");
    }

    #[test]
    fn tool_index_ranks_directory_listing_for_list_query() {
        let index = ToolIndex::from_tools(sample_tools());
        let hits = index.search("list directory folder path", 1);
        assert_eq!(hits[0].0, "list_directory");
    }
}
