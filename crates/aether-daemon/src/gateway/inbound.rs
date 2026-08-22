use crate::gateway::{
    discord, slack, telegram, GatewayChannelType, GatewayOutcome, GatewayRouter,
};
use crate::task_runner::run_gateway_inbound;
use crate::DaemonState;
use aether_permissions::GatewayGrant;
use crate::webhook_auth::env_channel_suffix;
use rusqlite::{params, Connection};

pub fn parse_payload(channel_type: GatewayChannelType, body: &str) -> Result<String, String> {
    match channel_type {
        GatewayChannelType::Slack => slack::parse_slack_payload(body),
        GatewayChannelType::Telegram => telegram::parse_telegram_payload(body),
        GatewayChannelType::Discord => discord::parse_discord_payload(body),
    }
}

fn allowed_sender(channel_id: &str) -> Option<String> {
    std::env::var(format!(
        "AETHER_GATEWAY_ALLOWED_SENDER_{}",
        env_channel_suffix(channel_id)
    ))
    .ok()
    .filter(|value| !value.is_empty())
}

fn remote_identity(
    channel_type: GatewayChannelType,
    body: &str,
) -> Result<(String, String), String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| format!("invalid gateway JSON: {error}"))?;
    match channel_type {
        GatewayChannelType::Slack => {
            let event_id = value
                .get("event_id")
                .and_then(|item| item.as_str())
                .ok_or("Slack payload missing event_id")?;
            let sender = value
                .pointer("/event/user")
                .and_then(|item| item.as_str())
                .ok_or("Slack payload missing event.user")?;
            Ok((event_id.to_string(), sender.to_string()))
        }
        GatewayChannelType::Telegram => {
            let event_id = value
                .get("update_id")
                .and_then(|item| item.as_i64())
                .ok_or("Telegram payload missing update_id")?;
            let sender = value
                .pointer("/message/from/id")
                .and_then(|item| item.as_i64())
                .ok_or("Telegram payload missing message.from.id")?;
            Ok((event_id.to_string(), sender.to_string()))
        }
        GatewayChannelType::Discord => Err(
            "Discord webhook disabled until Ed25519 verification is configured".into(),
        ),
    }
}

/// Bind a provider-authenticated event to an explicitly configured sender and claim its event id.
/// A duplicate provider id is rejected before any tool plan can execute.
pub fn authorize_and_claim_remote_event(
    conn: &Connection,
    channel_type: GatewayChannelType,
    channel_id: &str,
    body: &str,
) -> Result<(), String> {
    let expected_sender = allowed_sender(channel_id).ok_or_else(|| {
        format!(
            "allowed sender is not configured for channel {channel_id}; set AETHER_GATEWAY_ALLOWED_SENDER_{}",
            env_channel_suffix(channel_id)
        )
    })?;
    let (event_id, sender_id) = remote_identity(channel_type, body)?;
    if sender_id != expected_sender {
        return Err(format!("gateway sender {sender_id} is not authorized"));
    }
    conn.execute(
        "INSERT INTO gateway_events (channel_id, provider_event_id, sender_id)
         VALUES (?1, ?2, ?3)",
        params![channel_id, event_id, sender_id],
    )
    .map_err(|_| "gateway event replay detected".to_string())?;
    Ok(())
}

pub fn handle_inbound_post(
    conn: &Connection,
    channel_type: GatewayChannelType,
    channel_id: &str,
    body: &str,
) -> Result<GatewayOutcome, String> {
    let channel = GatewayRouter::load_channel(conn, channel_id)?
        .ok_or_else(|| format!("unknown gateway channel {}", channel_id))?;
    if channel.channel_type != channel_type {
        return Err(format!(
            "channel {} registered as {} but inbound was {}",
            channel_id,
            channel.channel_type.as_str(),
            channel_type.as_str()
        ));
    }
    let user_text = parse_payload(channel_type, body)?;
    let inbound = GatewayRouter::normalize_inbound(&channel, &user_text);
    GatewayRouter::handle_inbound(conn, &inbound)
}

pub fn handle_inbound_and_run(
    state: &DaemonState,
    channel_type: GatewayChannelType,
    channel_id: &str,
    body: &str,
) -> Result<(), String> {
    let conn = state.db.conn();
    let channel = GatewayRouter::load_channel(&conn, channel_id)?
        .ok_or_else(|| format!("unknown gateway channel {}", channel_id))?;

    match handle_inbound_post(&conn, channel_type, channel_id, body)? {
        GatewayOutcome::Denied { reason, .. } => Err(format!("denied: {}", reason)),
        GatewayOutcome::Accepted {
            normalized_prompt, ..
        } => {
            run_gateway_inbound(&conn, &channel, &normalized_prompt)?;
            GatewayGrant::audit_event(
                &conn,
                &channel.session_id,
                channel_id,
                "response",
                &aether_permissions::PermissionDecision::Approved,
                &serde_json::json!({"artifact": "gate_response.txt"}),
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_telegram_fixture_text() {
        let body = r#"{"message":{"text":"hi","chat":{"id":1}}}"#;
        let text = parse_payload(GatewayChannelType::Telegram, body).unwrap();
        assert_eq!(text, "hi");
    }

    #[test]
    fn remote_sender_is_bound_and_event_is_single_use() {
        let db = aether_db::Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('remote-s', 't', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO gateway_channels
             (channel_id, channel_type, session_id, task_prompt, enabled)
             VALUES ('remote-channel', 'telegram', 'remote-s', '{}', 1)",
            [],
        )
        .unwrap();
        let env_key = "AETHER_GATEWAY_ALLOWED_SENDER_REMOTE_CHANNEL";
        std::env::set_var(env_key, "42");
        let body = r#"{"update_id":7,"message":{"from":{"id":42},"chat":{"id":9},"text":"hi"}}"#;
        let first = authorize_and_claim_remote_event(
            &conn,
            GatewayChannelType::Telegram,
            "remote-channel",
            body,
        );
        let replay = authorize_and_claim_remote_event(
            &conn,
            GatewayChannelType::Telegram,
            "remote-channel",
            body,
        );
        std::env::remove_var(env_key);
        assert!(first.is_ok());
        assert!(replay.unwrap_err().contains("replay"));
    }
}
