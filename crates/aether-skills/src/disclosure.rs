//! BM25-lite progressive disclosure index for skills and chapters (Ratel borrow).
//!
//! Full ratel-mcp gateway deferred until LOOP-02 — see `docs/RATEL_TOOL_INDEX.md`.

use crate::SkillError;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

/// One searchable disclosure entry (skill root or chapter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisclosureEntry {
    pub id: String,
    pub kind: DisclosureKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisclosureKind {
    SkillRoot,
    SkillChapter,
}

/// In-process BM25 index over skill metadata and chapter text.
#[derive(Debug, Clone)]
pub struct DisclosureIndex {
    entries: Vec<DisclosureEntry>,
    tokenized: Vec<Vec<String>>,
    doc_freq: HashMap<String, usize>,
    avg_doc_len: f64,
}

impl DisclosureIndex {
    pub fn from_entries(entries: Vec<DisclosureEntry>) -> Self {
        let tokenized: Vec<Vec<String>> = entries.iter().map(|e| tokenize(&e.text)).collect();
        let mut doc_freq: HashMap<String, usize> = HashMap::new();

        for doc in &tokenized {
            let unique: std::collections::HashSet<&String> = doc.iter().collect();
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
            entries,
            tokenized,
            doc_freq,
            avg_doc_len,
        }
    }

    /// Build index from a progressive-disclosure skill directory (`SKILL.md` + `chapters/`).
    pub fn from_skill_dir(skill_dir: &Path) -> Result<Self, SkillError> {
        let skill_md = skill_dir.join("SKILL.md");
        let content = fs::read_to_string(&skill_md)?;
        let (frontmatter, body) = split_frontmatter_for_disclosure(&content)?;
        let meta = parse_frontmatter_for_disclosure(&frontmatter)?;

        let name = meta
            .get("name")
            .cloned()
            .ok_or_else(|| SkillError::Parse("Missing 'name' in SKILL.md frontmatter".into()))?;
        let description = meta
            .get("description")
            .cloned()
            .ok_or_else(|| SkillError::Parse("Missing 'description' in SKILL.md frontmatter".into()))?;
        let id = skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&name)
            .to_string();
        let routing_keywords = meta.get("routing_keywords").cloned().unwrap_or_default();

        let root_corpus = format!("{name} {description} {routing_keywords} {body}");
        let mut entries = vec![DisclosureEntry {
            id: id.clone(),
            kind: DisclosureKind::SkillRoot,
            text: root_corpus,
        }];

        let chapters_dir = skill_dir.join("chapters");
        if chapters_dir.is_dir() {
            let mut chapter_paths: Vec<PathBuf> = fs::read_dir(&chapters_dir)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
                .collect();
            chapter_paths.sort();

            for chapter_path in chapter_paths {
                let chapter_name = chapter_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("chapter")
                    .to_string();
                let chapter_text = fs::read_to_string(&chapter_path)?;
                let id = format!("{id}/{chapter_name}");
                entries.push(DisclosureEntry {
                    id,
                    kind: DisclosureKind::SkillChapter,
                    text: format!("{name} {chapter_name} {chapter_text}"),
                });
            }
        }

        Ok(Self::from_entries(entries))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entry(&self, index: usize) -> Option<&DisclosureEntry> {
        self.entries.get(index)
    }

    pub fn entry_by_id(&self, id: &str) -> Option<&DisclosureEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Return top-k `(entry_id, bm25_score)` for `query`, highest score first.
    pub fn search(&self, query: &str, k: usize) -> Vec<(String, f64)> {
        if self.entries.is_empty() || k == 0 {
            return Vec::new();
        }

        let query_terms = tokenize(query);
        if query_terms.is_empty() {
            return Vec::new();
        }

        let n_docs = self.entries.len() as f64;
        let mut scored: Vec<(usize, f64)> = (0..self.entries.len())
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
            .map(|(idx, score)| (self.entries[idx].id.clone(), score))
            .collect()
    }
}

fn split_frontmatter_for_disclosure(content: &str) -> Result<(String, &str), SkillError> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err(SkillError::Parse(
            "SKILL.md must begin with YAML frontmatter (---)".into(),
        ));
    }
    let rest = &trimmed[3..];
    let end = rest
        .find("\n---")
        .ok_or_else(|| SkillError::Parse("Unclosed YAML frontmatter".into()))?;
    Ok((rest[..end].trim().to_string(), &rest[end + 4..]))
}

fn parse_frontmatter_for_disclosure(
    yaml: &str,
) -> Result<HashMap<String, String>, SkillError> {
    let mut map = HashMap::new();
    for line in yaml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        map.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(map)
}

/// BM25 routing result for a progressive-disclosure chapter.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedChapter {
    pub chapter_path: String,
    pub score: f64,
    pub entry_id: String,
}

/// Route a natural-language query to the best matching chapter under `skill_dir`.
pub fn route_chapter_for_query(skill_dir: &Path, query: &str) -> Result<RoutedChapter, SkillError> {
    let index = DisclosureIndex::from_skill_dir(skill_dir)?;
    let hits = index.search(query, 5);

    for (entry_id, score) in hits {
        let Some(entry) = index.entry_by_id(&entry_id) else {
            continue;
        };
        if entry.kind != DisclosureKind::SkillChapter {
            continue;
        }
        let chapter_stem = entry_id
            .rsplit('/')
            .next()
            .ok_or_else(|| SkillError::Parse(format!("Invalid chapter entry id: {entry_id}")))?;
        return Ok(RoutedChapter {
            chapter_path: format!("chapters/{chapter_stem}.md"),
            score,
            entry_id,
        });
    }

    Err(SkillError::Parse(format!(
        "No chapter match for query in {}",
        skill_dir.display()
    )))
}

/// Normalize whitespace for citation comparison.
fn normalize_citation_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Fuzzy citation fidelity in `[0.0, 1.0]` — exact substring match yields 1.0.
pub fn citation_fidelity(citation: &str, answer: &str) -> f64 {
    let citation = normalize_citation_text(citation);
    let answer = normalize_citation_text(answer);

    if citation.is_empty() {
        return 0.0;
    }
    if answer.contains(&citation) {
        return 1.0;
    }

    let c_chars: Vec<char> = citation.chars().collect();
    let a_chars: Vec<char> = answer.chars().collect();
    let mut best = char_similarity(&citation, &answer);

    if a_chars.len() >= c_chars.len() {
        for window in a_chars.windows(c_chars.len()) {
            let window_str: String = window.iter().collect();
            best = best.max(char_similarity(&citation, &window_str));
        }
    }

    best
}

/// Build a citation-bearing answer line from chapter text (production path for SKILL-02).
pub fn compose_citation_answer(chapter_text: &str, citation_span: &str) -> Result<String, SkillError> {
    if !chapter_text.contains(citation_span) {
        return Err(SkillError::Parse(format!(
            "citation_span '{citation_span}' not found in chapter"
        )));
    }

    for line in chapter_text.lines() {
        if line.contains(citation_span) {
            return Ok(line.trim().to_string());
        }
    }

    Ok(citation_span.to_string())
}

fn char_similarity(a: &str, b: &str) -> f64 {
    let dist = levenshtein_chars(a, b);
    let max_len = a.chars().count().max(b.chars().count()).max(1);
    1.0 - (dist as f64 / max_len as f64)
}

fn levenshtein_chars(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
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

    fn fixture_dir(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/golden_harness/fixtures/skills")
            .join(name)
    }

    #[test]
    fn bm25_ranks_async_chapter_for_tokio_query() {
        let index = DisclosureIndex::from_skill_dir(&fixture_dir("rust-cookbook")).unwrap();
        assert!(index.len() >= 4, "root + 3 chapters");

        let hits = index.search("tokio async runtime spawn", 3);
        assert!(!hits.is_empty());
        assert!(
            hits[0].0.contains("02-async-runtime"),
            "expected async chapter top-1, got {:?}",
            hits
        );
    }

    #[test]
    fn bm25_ranks_error_chapter_for_result_query() {
        let index = DisclosureIndex::from_skill_dir(&fixture_dir("rust-cookbook")).unwrap();
        let hits = index.search("Result thiserror question mark propagate", 2);
        assert!(!hits.is_empty());
        assert!(
            hits.iter()
                .any(|(id, _)| id.contains("01-error-handling")),
            "expected error-handling chapter in top-2, got {:?}",
            hits
        );
    }

    #[test]
    fn book_skill_routes_daemon_bind_question() {
        let index = DisclosureIndex::from_skill_dir(&fixture_dir("book_skill")).unwrap();
        let hits = index.search(
            "What is the default TCP listen address for aether-daemon?",
            3,
        );
        assert!(!hits.is_empty());
        assert!(
            hits[0].0.contains("01-daemon-lifecycle"),
            "expected daemon chapter top-1, got {:?}",
            hits
        );
    }

    #[test]
    fn book_skill_routes_ingest_audit_question() {
        let routed = route_chapter_for_query(
            &fixture_dir("book_skill"),
            "What audit tool name is recorded when graph ingest fails?",
        )
        .unwrap();
        assert_eq!(routed.chapter_path, "chapters/02-graph-ingest.md");
    }

    #[test]
    fn citation_fidelity_exact_substring_is_one() {
        assert_eq!(
            citation_fidelity("127.0.0.1:7878", "binds to 127.0.0.1:7878 by default"),
            1.0
        );
    }

    #[test]
    fn synthetic_entries_return_top_k_by_relevance() {
        let entries = vec![
            DisclosureEntry {
                id: "alpha".into(),
                kind: DisclosureKind::SkillRoot,
                text: "filesystem read write directory".into(),
            },
            DisclosureEntry {
                id: "beta".into(),
                kind: DisclosureKind::SkillRoot,
                text: "python lint syntax checker".into(),
            },
            DisclosureEntry {
                id: "gamma".into(),
                kind: DisclosureKind::SkillRoot,
                text: "git commit branch merge".into(),
            },
        ];
        let index = DisclosureIndex::from_entries(entries);
        let hits = index.search("python syntax lint", 2);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].0, "beta");
    }
}
