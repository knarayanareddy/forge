use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ToolCallCase {
    pub id: String,
    pub tool_name: String,
    pub expect_success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FrozenToolResponse {
    pub tool_name: String,
    pub success: bool,
    pub arguments_match: bool,
    #[serde(default)]
    pub output: String,
}

pub fn score_tool_response(case: &ToolCallCase, response: &FrozenToolResponse) -> f64 {
    let mut score = 0.0;
    if response.tool_name == case.tool_name {
        score += 0.4;
    }
    if response.success == case.expect_success {
        score += 0.35;
    }
    if response.arguments_match {
        score += 0.25;
    }
    score
}

pub fn evaluate_profile_reliability(
    cases: &[ToolCallCase],
    responses: &HashMap<String, FrozenToolResponse>,
) -> f64 {
    if cases.is_empty() {
        return 0.0;
    }
    cases
        .iter()
        .map(|c| {
            responses
                .get(&c.id)
                .map(|r| score_tool_response(c, r))
                .unwrap_or(0.0)
        })
        .sum::<f64>()
        / cases.len() as f64
}

pub fn rank_profiles_by_reliability(
    profiles: &[(String, HashMap<String, FrozenToolResponse>)],
    cases: &[ToolCallCase],
) -> Vec<(String, f64)> {
    let mut ranked: Vec<(String, f64)> = profiles
        .iter()
        .map(|(id, r)| (id.clone(), evaluate_profile_reliability(cases, r)))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q8_outranks_q4() {
        let cases = vec![
            ToolCallCase {
                id: "a".into(),
                tool_name: "fs_write".into(),
                expect_success: true,
            },
            ToolCallCase {
                id: "b".into(),
                tool_name: "fs_read".into(),
                expect_success: true,
            },
        ];
        let q4 = HashMap::from([
            (
                "a".into(),
                FrozenToolResponse {
                    tool_name: "fs_write".into(),
                    success: true,
                    arguments_match: false,
                    output: String::new(),
                },
            ),
            (
                "b".into(),
                FrozenToolResponse {
                    tool_name: "fs_read".into(),
                    success: false,
                    arguments_match: true,
                    output: String::new(),
                },
            ),
        ]);
        let q8 = HashMap::from([
            (
                "a".into(),
                FrozenToolResponse {
                    tool_name: "fs_write".into(),
                    success: true,
                    arguments_match: true,
                    output: String::new(),
                },
            ),
            (
                "b".into(),
                FrozenToolResponse {
                    tool_name: "fs_read".into(),
                    success: true,
                    arguments_match: true,
                    output: String::new(),
                },
            ),
        ]);
        assert_eq!(
            rank_profiles_by_reliability(&[("q4".into(), q4), ("q8".into(), q8)], &cases)[0].0,
            "q8"
        );
    }
}
