//! Graph v2 — bounded multi-hop traversal with bidirectional edges and recency decay.

use crate::graph::{GraphEdge, GraphNeighbor};
use crate::Database;
use rusqlite::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// Default recency decay λ (per day) for edge weights: weight *= exp(-λ * age_days).
pub const DEFAULT_DECAY_LAMBDA: f64 = 0.05;

/// Apply recency decay to an edge weight based on `recorded_at` timestamp.
pub fn decay_edge_weight(weight: f64, recorded_at: &str, lambda: f64, now_days: f64) -> f64 {
    let age_days = parse_sqlite_age_days(recorded_at, now_days);
    weight * (-lambda * age_days).exp()
}

fn parse_sqlite_age_days(recorded_at: &str, now_days: f64) -> f64 {
    if recorded_at.len() < 10 {
        return 0.0;
    }
    let date_part = &recorded_at[..10];
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.len() != 3 {
        return 0.0;
    }
    let year: f64 = parts[0].parse().unwrap_or(2026.0);
    let month: f64 = parts[1].parse().unwrap_or(1.0);
    let day: f64 = parts[2].parse().unwrap_or(1.0);
    let recorded_days = year * 365.25 + month * 30.0 + day;
    (now_days - recorded_days).max(0.0)
}

fn current_day_index() -> f64 {
    2026.0 * 365.25 + 8.0 * 30.0 + 4.0
}

impl Database {
    /// Bidirectional 1-hop neighbors (outgoing + incoming edges).
    pub fn get_bidirectional_one_hop_neighbors(
        &self,
        session_id: &str,
        src_node_ids: &[&str],
        as_of: Option<&str>,
    ) -> Result<Vec<GraphNeighbor>> {
        let mut seen = HashSet::new();
        let mut combined = Vec::new();

        for neighbor in self.get_one_hop_neighbors(session_id, src_node_ids, as_of)? {
            let key = (neighbor.edge.src_node_id.clone(), neighbor.edge.dst_node_id.clone());
            if seen.insert(key) {
                combined.push(neighbor);
            }
        }

        for src in src_node_ids {
            let reverse = self.get_incoming_neighbors(session_id, &[*src], as_of)?;
            for neighbor in reverse {
                let key = (neighbor.edge.src_node_id.clone(), neighbor.edge.dst_node_id.clone());
                if seen.insert(key) {
                    combined.push(neighbor);
                }
            }
        }

        Ok(combined)
    }

    fn get_incoming_neighbors(
        &self,
        session_id: &str,
        dst_node_ids: &[&str],
        as_of: Option<&str>,
    ) -> Result<Vec<GraphNeighbor>> {
        if dst_node_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: String = std::iter::repeat("?")
            .take(dst_node_ids.len())
            .collect::<Vec<_>>()
            .join(", ");

        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT DISTINCT e.id, e.session_id, e.src_node_id, e.dst_node_id,
                    e.relation_type, e.weight, e.evidence_text, e.source_uri,
                    e.valid_from, e.valid_to, e.recorded_at,
                    n.canonical_name AS dst_name
             FROM graph_edges e
             JOIN graph_nodes n ON n.id = e.src_node_id
             WHERE e.session_id = ?1
               AND e.dst_node_id IN ({placeholders})
               AND e.valid_from <= {as_of_expr}
               AND (e.valid_to IS NULL OR e.valid_to > {as_of_expr})
               AND n.valid_from <= {as_of_expr}
               AND (n.valid_to IS NULL OR n.valid_to > {as_of_expr})
               AND n.superseded_by IS NULL
             ORDER BY e.weight DESC, n.canonical_name",
            as_of_expr = if as_of.is_some() { "?2" } else { "datetime('now')" },
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut query_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        query_params.push(Box::new(session_id.to_string()));
        if let Some(ts) = as_of {
            query_params.push(Box::new(ts.to_string()));
        }
        for id in dst_node_ids {
            query_params.push(Box::new(id.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            query_params.iter().map(|p| p.as_ref()).collect();

        let mut rows = stmt.query(param_refs.as_slice())?;
        let mut neighbors = Vec::new();
        while let Some(row) = rows.next()? {
            neighbors.push(GraphNeighbor {
                edge: GraphEdge {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    src_node_id: row.get(2)?,
                    dst_node_id: row.get(3)?,
                    relation_type: row.get(4)?,
                    weight: row.get(5)?,
                    evidence_text: row.get(6)?,
                    source_uri: row.get(7)?,
                    valid_from: row.get(8)?,
                    valid_to: row.get(9)?,
                    recorded_at: row.get(10)?,
                },
                dst_name: row.get(11)?,
            });
        }
        Ok(neighbors)
    }

    /// Bounded multi-hop expansion with path-score product and recency decay (Phase 11.13).
    pub fn expand_graph_neighbors_v2(
        &self,
        session_id: &str,
        seed_node_ids: &[&str],
        max_depth: i32,
        max_expansion: usize,
        decay_lambda: f64,
    ) -> Result<Vec<(String, f64)>> {
        if seed_node_ids.is_empty() || max_depth <= 0 || max_expansion == 0 {
            return Ok(Vec::new());
        }

        let now_days = current_day_index();
        let mut best_scores: HashMap<String, f64> = HashMap::new();
        let mut frontier: VecDeque<(String, f64, i32)> = VecDeque::new();
        let mut visited: HashSet<String> = HashSet::new();

        for seed in seed_node_ids {
            frontier.push_back((seed.to_string(), 1.0, 0));
        }

        while let Some((node_id, path_score, depth)) = frontier.pop_front() {
            if best_scores.len() >= max_expansion && depth > 0 {
                continue;
            }

            if depth >= max_depth {
                continue;
            }

            let neighbors = self.get_bidirectional_one_hop_neighbors(
                session_id,
                &[node_id.as_str()],
                None,
            )?;

            for neighbor in neighbors {
                let next_id = if neighbor.edge.src_node_id == node_id {
                    neighbor.edge.dst_node_id.clone()
                } else {
                    neighbor.edge.src_node_id.clone()
                };

                if seed_node_ids.contains(&next_id.as_str()) {
                    continue;
                }

                let decayed = decay_edge_weight(
                    neighbor.edge.weight,
                    &neighbor.edge.recorded_at,
                    decay_lambda,
                    now_days,
                );
                let expanded_score = path_score * decayed;

                let entry = best_scores.entry(next_id.clone()).or_insert(0.0);
                if expanded_score > *entry {
                    *entry = expanded_score;
                }

                let visit_key = format!("{}:{}", next_id, depth + 1);
                if visited.insert(visit_key) && depth + 1 < max_depth {
                    frontier.push_back((next_id, expanded_score, depth + 1));
                }
            }
        }

        let mut scored: Vec<(String, f64)> = best_scores.into_iter().collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(max_expansion);
        Ok(scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EntityType, NewGraphEdge, NewGraphNode, RelationType};

    #[test]
    fn decay_reduces_old_edge_weight() {
        let fresh = decay_edge_weight(2.0, "2026-08-04 12:00:00", 0.05, current_day_index());
        let old = decay_edge_weight(2.0, "2026-01-01 12:00:00", 0.05, current_day_index());
        assert!(fresh >= old);
        assert!(old < 2.0);
    }

    #[test]
    fn multi_hop_reaches_two_hop_node() {
        let db = Database::open_in_memory().unwrap();
        {
            let conn = db.conn();
            conn.execute(
                "INSERT INTO sessions (id, title, status) VALUES ('sess-v2', 'v2', 'active')",
                [],
            )
            .unwrap();
        }
        db.insert_graph_node(NewGraphNode {
            id: "n-a",
            session_id: "sess-v2",
            entity_type: EntityType::Concept,
            canonical_name: "A",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "test",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();
        for (id, name) in [("n-b", "B"), ("n-c", "C")] {
            db.insert_graph_node(NewGraphNode {
                id,
                session_id: "sess-v2",
                entity_type: EntityType::Concept,
                canonical_name: name,
                aliases_json: "[]",
                properties_json: "{}",
                source_uri: "test",
                valid_from: None,
                valid_to: None,
            })
            .unwrap();
        }
        db.insert_graph_edge(NewGraphEdge {
            session_id: "sess-v2",
            src_node_id: "n-a",
            dst_node_id: "n-b",
            relation_type: RelationType::RelatedTo,
            weight: 2.0,
            evidence_text: "a-b",
            source_uri: "test",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();
        db.insert_graph_edge(NewGraphEdge {
            session_id: "sess-v2",
            src_node_id: "n-b",
            dst_node_id: "n-c",
            relation_type: RelationType::RelatedTo,
            weight: 2.0,
            evidence_text: "b-c",
            source_uri: "test",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        let scored = db
            .expand_graph_neighbors_v2("sess-v2", &["n-a"], 2, 16, DEFAULT_DECAY_LAMBDA)
            .unwrap();
        let ids: Vec<&str> = scored.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"n-b"));
        assert!(ids.contains(&"n-c"));
    }
}
