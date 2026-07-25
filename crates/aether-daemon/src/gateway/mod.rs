pub mod discord;
pub mod mock_server;
pub mod slack;
pub mod telegram;

use aether_permissions::{GatewayGrant, PermissionDecision};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayChannelType {
    Slack,
    Telegram,
    Discord,
}

impl GatewayChannelType {
    pub fn as_str(self) -> &'static str {
        match self {
            GatewayChannelType::Slack => "slack",
            GatewayChannelType::Telegram => "telegram",
            GatewayChannelType::Discord => "discord",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "slack" => Some(GatewayChannelType::Slack),
            "telegram" => Some(GatewayChannelType::Telegram),
            "discord" => Some(GatewayChannelType::Discord),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GatewayChannel {
    pub channel_id: String,
    pub channel_type: GatewayChannelType,
    pub session_id: String,
    pub task_prompt: String,
    pub workspace_path: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct InboundGatewayMessage {
    pub channel_id: String,
    pub channel_type: GatewayChannelType,
    pub session_id: String,
    pub user_text: String,
    pub normalized_prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayOutcome {
    Denied { channel_id: String, reason: String },
    Accepted {
        channel_id: String,
        normalized_prompt: String,
    },
}

pub struct GatewayRouter;

impl GatewayRouter {
    pub fn register_channel(conn: &Connection, channel: &GatewayChannel) -> Result<(), String> {
        conn.execute(
            "INSERT OR REPLACE INTO gateway_channels
             (channel_id, channel_type, session_id, task_prompt, workspace_path, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                channel.channel_id,
                channel.channel_type.as_str(),
                channel.session_id,
                channel.task_prompt,
                channel.workspace_path,
                channel.enabled as i32,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_channel(conn: &Connection, channel_id: &str) -> Result<Option<GatewayChannel>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT channel_id, channel_type, session_id, task_prompt, workspace_path, enabled
                 FROM gateway_channels WHERE channel_id = ?1",
            )
            .map_err(|e| e.to_string())?;

        let mut rows = stmt
            .query_map(params![channel_id], |row| {
                Ok(GatewayChannel {
                    channel_id: row.get(0)?,
                    channel_type: GatewayChannelType::parse(&row.get::<_, String>(1)?)
                        .unwrap_or(GatewayChannelType::Slack),
                    session_id: row.get(2)?,
                    task_prompt: row.get(3)?,
                    workspace_path: row.get(4)?,
                    enabled: row.get::<_, i32>(5)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;

        Ok(rows.next().transpose().map_err(|e| e.to_string())?)
    }

    /// Grant gate before any inbound message reaches the loop (RED-01 extended).
    pub fn handle_inbound(
        conn: &Connection,
        inbound: &InboundGatewayMessage,
    ) -> Result<GatewayOutcome, String> {
        let channel = Self::load_channel(conn, &inbound.channel_id)?
            .ok_or_else(|| format!("unknown gateway channel {}", inbound.channel_id))?;

        if !channel.enabled {
            GatewayGrant::audit_event(
                conn,
                &inbound.session_id,
                &inbound.channel_id,
                "inbound",
                &PermissionDecision::Denied,
                &serde_json::json!({"reason": "channel disabled"}),
            )
            .map_err(|e| e.to_string())?;
            return Ok(GatewayOutcome::Denied {
                channel_id: inbound.channel_id.clone(),
                reason: "channel disabled".into(),
            });
        }

        let grant = GatewayGrant::check(conn, &inbound.channel_id, &inbound.session_id)
            .map_err(|e| e.to_string())?;
        if grant != PermissionDecision::Approved {
            GatewayGrant::audit_event(
                conn,
                &inbound.session_id,
                &inbound.channel_id,
                "inbound",
                &PermissionDecision::Denied,
                &serde_json::json!({"reason": "missing GatewayGrant"}),
            )
            .map_err(|e| e.to_string())?;
            return Ok(GatewayOutcome::Denied {
                channel_id: inbound.channel_id.clone(),
                reason: "missing GatewayGrant".into(),
            });
        }

        GatewayGrant::audit_event(
            conn,
            &inbound.session_id,
            &inbound.channel_id,
            "inbound",
            &PermissionDecision::Approved,
            &serde_json::json!({
                "source": inbound.channel_type.as_str(),
                "text_len": inbound.user_text.len(),
            }),
        )
        .map_err(|e| e.to_string())?;

        Ok(GatewayOutcome::Accepted {
            channel_id: inbound.channel_id.clone(),
            normalized_prompt: inbound.normalized_prompt.clone(),
        })
    }

    pub fn normalize_inbound(
        channel: &GatewayChannel,
        user_text: &str,
    ) -> InboundGatewayMessage {
        let normalized_prompt = match channel.channel_type {
            GatewayChannelType::Slack => slack::normalize_message(&channel.task_prompt, user_text),
            GatewayChannelType::Telegram => telegram::normalize_message(&channel.task_prompt, user_text),
            GatewayChannelType::Discord => discord::normalize_message(&channel.task_prompt, user_text),
        };

        InboundGatewayMessage {
            channel_id: channel.channel_id.clone(),
            channel_type: channel.channel_type,
            session_id: channel.session_id.clone(),
            user_text: user_text.to_string(),
            normalized_prompt,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_denied_without_grant() {
        let db = aether_db::Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('sess-gate', 'Gate', 'active')",
            [],
        )
        .unwrap();

        let channel = GatewayChannel {
            channel_id: "slack-gate-01".into(),
            channel_type: GatewayChannelType::Slack,
            session_id: "sess-gate".into(),
            task_prompt: r#"{"loop":[{"action":"done"}]}"#.into(),
            workspace_path: None,
            enabled: true,
        };
        GatewayRouter::register_channel(&conn, &channel).unwrap();

        let inbound = GatewayRouter::normalize_inbound(&channel, "hello");
        let outcome = GatewayRouter::handle_inbound(&conn, &inbound).unwrap();
        assert!(matches!(outcome, GatewayOutcome::Denied { .. }));

        let denied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log
                 WHERE tool_name = 'gateway_inbound' AND decision = 'denied'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(denied, 1);
    }
}
