use crate::gateway::{
    discord, slack, telegram, GatewayChannelType, GatewayOutcome, GatewayRouter,
};
use crate::task_runner::run_gateway_inbound;
use crate::DaemonState;
use aether_permissions::GatewayGrant;
use rusqlite::Connection;

pub fn parse_payload(channel_type: GatewayChannelType, body: &str) -> Result<String, String> {
    match channel_type {
        GatewayChannelType::Slack => slack::parse_slack_payload(body),
        GatewayChannelType::Telegram => telegram::parse_telegram_payload(body),
        GatewayChannelType::Discord => discord::parse_discord_payload(body),
    }
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
}
