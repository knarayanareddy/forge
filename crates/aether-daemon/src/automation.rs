use aether_permissions::{AutomationGrant, PermissionDecision};
use rusqlite::{Connection, Result, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    Cron,
    FileWatch,
    PrWebhook,
}

impl TriggerType {
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerType::Cron => "cron",
            TriggerType::FileWatch => "file_watch",
            TriggerType::PrWebhook => "pr_webhook",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "cron" => Some(TriggerType::Cron),
            "file_watch" => Some(TriggerType::FileWatch),
            "pr_webhook" => Some(TriggerType::PrWebhook),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TriggerConfig {
    #[serde(default)]
    pub interval_secs: Option<u64>,
    #[serde(default)]
    pub watch_path: Option<String>,
    #[serde(default)]
    pub webhook_secret: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AutomationTrigger {
    pub trigger_id: String,
    pub trigger_type: TriggerType,
    pub session_id: String,
    pub config: TriggerConfig,
    pub task_prompt: String,
    pub workspace_path: Option<String>,
    pub enabled: bool,
    pub last_fired_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationOutcome {
    /// Slice 7.1: cron tick recorded in audit_log only.
    Audited { trigger_id: String },
    /// Slice 7.2+: trigger enqueued for execution.
    Enqueued { trigger_id: String, queue_id: i64 },
    Denied { trigger_id: String, reason: String },
    Skipped { trigger_id: String, reason: String },
}

pub struct AutomationScheduler {
    file_snapshots: HashMap<String, u64>,
}

impl Default for AutomationScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl AutomationScheduler {
    pub fn new() -> Self {
        Self {
            file_snapshots: HashMap::new(),
        }
    }

    pub fn register_trigger(
        conn: &Connection,
        trigger: &AutomationTrigger,
    ) -> Result<(), String> {
        conn.execute(
            "INSERT OR REPLACE INTO automation_triggers
             (trigger_id, trigger_type, session_id, config_json, task_prompt, workspace_path, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                trigger.trigger_id,
                trigger.trigger_type.as_str(),
                trigger.session_id,
                serde_json::to_string(&trigger.config).map_err(|e| e.to_string())?,
                trigger.task_prompt,
                trigger.workspace_path,
                trigger.enabled as i32,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_trigger(conn: &Connection, trigger_id: &str) -> Result<Option<AutomationTrigger>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT trigger_id, trigger_type, session_id, config_json, task_prompt,
                        workspace_path, enabled, last_fired_at
                 FROM automation_triggers WHERE trigger_id = ?1",
            )
            .map_err(|e| e.to_string())?;

        let mut rows = stmt
            .query_map(params![trigger_id], |row| {
                let config_json: String = row.get(3)?;
                let config: TriggerConfig = serde_json::from_str(&config_json).unwrap_or_default();
                let last_fired: Option<String> = row.get(7)?;
                Ok(AutomationTrigger {
                    trigger_id: row.get(0)?,
                    trigger_type: TriggerType::parse(&row.get::<_, String>(1)?)
                        .unwrap_or(TriggerType::Cron),
                    session_id: row.get(2)?,
                    config,
                    task_prompt: row.get(4)?,
                    workspace_path: row.get(5)?,
                    enabled: row.get::<_, i32>(6)? != 0,
                    last_fired_at: parse_db_timestamp(last_fired),
                })
            })
            .map_err(|e| e.to_string())?;

        Ok(rows.next().transpose().map_err(|e| e.to_string())?)
    }

    /// Slice 7.1 ship signal: evaluate due cron triggers and write audit entries.
    pub fn tick_cron(
        &mut self,
        conn: &Connection,
        now: SystemTime,
    ) -> Result<Vec<AutomationOutcome>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT trigger_id, trigger_type, session_id, config_json, task_prompt,
                        workspace_path, enabled, last_fired_at
                 FROM automation_triggers
                 WHERE enabled = 1 AND trigger_type = 'cron'",
            )
            .map_err(|e| e.to_string())?;

        let triggers: Vec<AutomationTrigger> = stmt
            .query_map([], |row| {
                let config_json: String = row.get(3)?;
                let config: TriggerConfig = serde_json::from_str(&config_json).unwrap_or_default();
                let last_fired: Option<String> = row.get(7)?;
                Ok(AutomationTrigger {
                    trigger_id: row.get(0)?,
                    trigger_type: TriggerType::Cron,
                    session_id: row.get(2)?,
                    config,
                    task_prompt: row.get(4)?,
                    workspace_path: row.get(5)?,
                    enabled: true,
                    last_fired_at: parse_db_timestamp(last_fired),
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        let mut outcomes = Vec::new();
        for trigger in triggers {
            if !cron_is_due(&trigger, now) {
                outcomes.push(AutomationOutcome::Skipped {
                    trigger_id: trigger.trigger_id.clone(),
                    reason: "not due".into(),
                });
                continue;
            }
            outcomes.push(self.fire_trigger(conn, &trigger, "cron", false)?);
        }
        Ok(outcomes)
    }

    /// Slice 7.2: poll watched paths and enqueue on mtime change.
    pub fn poll_file_watchers(
        &mut self,
        conn: &Connection,
    ) -> Result<Vec<AutomationOutcome>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT trigger_id, trigger_type, session_id, config_json, task_prompt,
                        workspace_path, enabled, last_fired_at
                 FROM automation_triggers
                 WHERE enabled = 1 AND trigger_type = 'file_watch'",
            )
            .map_err(|e| e.to_string())?;

        let triggers: Vec<AutomationTrigger> = stmt
            .query_map([], |row| {
                let config_json: String = row.get(3)?;
                let config: TriggerConfig = serde_json::from_str(&config_json).unwrap_or_default();
                let last_fired: Option<String> = row.get(7)?;
                Ok(AutomationTrigger {
                    trigger_id: row.get(0)?,
                    trigger_type: TriggerType::FileWatch,
                    session_id: row.get(2)?,
                    config,
                    task_prompt: row.get(4)?,
                    workspace_path: row.get(5)?,
                    enabled: true,
                    last_fired_at: parse_db_timestamp(last_fired),
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        let mut outcomes = Vec::new();
        for trigger in triggers {
            let watch_path = match trigger.config.watch_path.as_deref() {
                Some(p) => p.to_string(),
                None => {
                    outcomes.push(AutomationOutcome::Skipped {
                        trigger_id: trigger.trigger_id.clone(),
                        reason: "missing watch_path".into(),
                    });
                    continue;
                }
            };

            let mtime = file_mtime_secs(&watch_path)?;
            let prev = self.file_snapshots.get(&trigger.trigger_id).copied();
            self.file_snapshots.insert(trigger.trigger_id.clone(), mtime);

            if prev.is_none() {
                outcomes.push(AutomationOutcome::Skipped {
                    trigger_id: trigger.trigger_id.clone(),
                    reason: "baseline snapshot".into(),
                });
                continue;
            }

            if prev == Some(mtime) {
                outcomes.push(AutomationOutcome::Skipped {
                    trigger_id: trigger.trigger_id.clone(),
                    reason: "no change".into(),
                });
                continue;
            }

            outcomes.push(self.fire_trigger(conn, &trigger, "file_watch", true)?);
        }
        Ok(outcomes)
    }

    /// Slice 7.2: PR webhook stub — validate optional secret and enqueue.
    pub fn handle_pr_webhook(
        &mut self,
        conn: &Connection,
        trigger_id: &str,
        payload: &str,
        provided_secret: Option<&str>,
    ) -> Result<AutomationOutcome, String> {
        let trigger = Self::load_trigger(conn, trigger_id)?
            .ok_or_else(|| format!("unknown trigger_id {}", trigger_id))?;

        if trigger.trigger_type != TriggerType::PrWebhook {
            return Err(format!("trigger {} is not pr_webhook", trigger_id));
        }

        if let Some(expected) = trigger.config.webhook_secret.as_deref() {
            if provided_secret != Some(expected) {
                AutomationGrant::audit_event(
                    conn,
                    &trigger.session_id,
                    trigger_id,
                    "webhook_denied",
                    &PermissionDecision::Denied,
                    &serde_json::json!({"reason": "invalid webhook secret"}),
                )
                .map_err(|e| e.to_string())?;
                return Ok(AutomationOutcome::Denied {
                    trigger_id: trigger_id.to_string(),
                    reason: "invalid webhook secret".into(),
                });
            }
        }

        let _: serde_json::Value =
            serde_json::from_str(payload).map_err(|e| format!("invalid webhook JSON: {}", e))?;

        self.fire_trigger(
            conn,
            &trigger,
            "pr_webhook",
            true,
        )
    }

    /// Enqueue a trigger for execution (used by AUTO-01 harness and daemon run loop).
    pub fn enqueue_trigger(
        &mut self,
        conn: &Connection,
        trigger: &AutomationTrigger,
        source: &str,
    ) -> Result<AutomationOutcome, String> {
        self.fire_trigger(conn, trigger, source, true)
    }

    /// Enqueue a trigger for run_task execution (AUTO-01 / slice 7.3 production path).
    pub fn trigger_automation_run(
        &mut self,
        conn: &Connection,
        trigger: &AutomationTrigger,
        source: &str,
    ) -> Result<AutomationOutcome, String> {
        self.fire_trigger(conn, trigger, source, true)
    }

    fn fire_trigger(
        &mut self,
        conn: &Connection,
        trigger: &AutomationTrigger,
        source: &str,
        enqueue: bool,
    ) -> Result<AutomationOutcome, String> {
        let grant = AutomationGrant::check(conn, &trigger.trigger_id, &trigger.session_id)
            .map_err(|e| e.to_string())?;

        if grant != PermissionDecision::Approved {
            AutomationGrant::audit_event(
                conn,
                &trigger.session_id,
                &trigger.trigger_id,
                "denied",
                &PermissionDecision::Denied,
                &serde_json::json!({"source": source, "reason": "missing AutomationGrant"}),
            )
            .map_err(|e| e.to_string())?;
            return Ok(AutomationOutcome::Denied {
                trigger_id: trigger.trigger_id.clone(),
                reason: "missing AutomationGrant".into(),
            });
        }

        AutomationGrant::audit_event(
            conn,
            &trigger.session_id,
            &trigger.trigger_id,
            if enqueue { "enqueue" } else { "tick" },
            &PermissionDecision::Approved,
            &serde_json::json!({"source": source}),
        )
        .map_err(|e| e.to_string())?;

        mark_trigger_fired(conn, &trigger.trigger_id).map_err(|e| e.to_string())?;

        if !enqueue {
            return Ok(AutomationOutcome::Audited {
                trigger_id: trigger.trigger_id.clone(),
            });
        }

        conn.execute(
            "INSERT INTO automation_queue (trigger_id, session_id, status, detail_json)
             VALUES (?1, ?2, 'pending', ?3)",
            params![
                trigger.trigger_id,
                trigger.session_id,
                serde_json::json!({"source": source}).to_string(),
            ],
        )
        .map_err(|e| e.to_string())?;
        let queue_id = conn.last_insert_rowid();

        Ok(AutomationOutcome::Enqueued {
            trigger_id: trigger.trigger_id.clone(),
            queue_id,
        })
    }

    /// Slice 7.3: dequeue pending automation and execute via provided runner.
    pub fn run_pending<F>(
        conn: &Connection,
        max_runs: usize,
        mut runner: F,
    ) -> Result<Vec<(i64, Result<(), String>)>, String>
    where
        F: FnMut(&AutomationTrigger) -> Result<(), String>,
    {
        let mut stmt = conn
            .prepare(
                "SELECT q.id, t.trigger_id, t.trigger_type, t.session_id, t.config_json,
                        t.task_prompt, t.workspace_path, t.enabled, t.last_fired_at
                 FROM automation_queue q
                 JOIN automation_triggers t ON t.trigger_id = q.trigger_id
                 WHERE q.status = 'pending'
                 ORDER BY q.id ASC
                 LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;

        let rows: Vec<(i64, AutomationTrigger)> = stmt
            .query_map(params![max_runs as i64], |row| {
                let config_json: String = row.get(4)?;
                let config: TriggerConfig = serde_json::from_str(&config_json).unwrap_or_default();
                let trigger_type = TriggerType::parse(&row.get::<_, String>(2)?)
                    .unwrap_or(TriggerType::Cron);
                let last_fired: Option<String> = row.get(8)?;
                Ok((
                    row.get(0)?,
                    AutomationTrigger {
                        trigger_id: row.get(1)?,
                        trigger_type,
                        session_id: row.get(3)?,
                        config,
                        task_prompt: row.get(5)?,
                        workspace_path: row.get(6)?,
                        enabled: row.get::<_, i32>(7)? != 0,
                        last_fired_at: parse_db_timestamp(last_fired),
                    },
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for (queue_id, trigger) in rows {
            let grant = AutomationGrant::check(conn, &trigger.trigger_id, &trigger.session_id)
                .map_err(|e| e.to_string())?;
            if grant != PermissionDecision::Approved {
                conn.execute(
                    "UPDATE automation_queue SET status = 'denied', finished_at = CURRENT_TIMESTAMP
                     WHERE id = ?1",
                    params![queue_id],
                )
                .map_err(|e| e.to_string())?;
                results.push((queue_id, Err("missing AutomationGrant".into())));
                continue;
            }

            conn.execute(
                "UPDATE automation_queue SET status = 'running', started_at = CURRENT_TIMESTAMP
                 WHERE id = ?1",
                params![queue_id],
            )
            .map_err(|e| e.to_string())?;

            let run_result = runner(&trigger);
            let status = if run_result.is_ok() { "completed" } else { "failed" };
            conn.execute(
                "UPDATE automation_queue SET status = ?1, finished_at = CURRENT_TIMESTAMP WHERE id = ?2",
                params![status, queue_id],
            )
            .map_err(|e| e.to_string())?;

            if run_result.is_ok() {
                AutomationGrant::audit_event(
                    conn,
                    &trigger.session_id,
                    &trigger.trigger_id,
                    "completed",
                    &PermissionDecision::Approved,
                    &serde_json::json!({"queue_id": queue_id}),
                )
                .map_err(|e| e.to_string())?;
            }

            results.push((queue_id, run_result));
        }

        Ok(results)
    }
}

fn cron_is_due(trigger: &AutomationTrigger, now: SystemTime) -> bool {
    let interval = trigger
        .config
        .interval_secs
        .unwrap_or(60)
        .max(1);
    match trigger.last_fired_at {
        None => true,
        Some(last) => now
            .duration_since(last)
            .unwrap_or(Duration::ZERO)
            >= Duration::from_secs(interval),
    }
}

fn mark_trigger_fired(conn: &Connection, trigger_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE automation_triggers SET last_fired_at = CURRENT_TIMESTAMP WHERE trigger_id = ?1",
        params![trigger_id],
    )?;
    Ok(())
}

fn parse_db_timestamp(raw: Option<String>) -> Option<SystemTime> {
    raw.and_then(|s| {
        // SQLite CURRENT_TIMESTAMP is UTC "YYYY-MM-DD HH:MM:SS"
        chrono_like_parse(&s).or_else(|| {
            s.parse::<u64>()
                .ok()
                .and_then(|secs| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs)))
        })
    })
}

fn chrono_like_parse(s: &str) -> Option<SystemTime> {
    let parts: Vec<&str> = s.split([' ', 'T']).collect();
    if parts.len() < 2 {
        return None;
    }
    let date: Vec<&str> = parts[0].split('-').collect();
    let time: Vec<&str> = parts[1].split(':').collect();
    if date.len() != 3 || time.len() < 2 {
        return None;
    }
    let year: i64 = date[0].parse().ok()?;
    let month: u64 = date[1].parse().ok()?;
    let day: u64 = date[2].parse().ok()?;
    let hour: u64 = time[0].parse().ok()?;
    let minute: u64 = time[1].parse().ok()?;
    let second: u64 = time.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);

    let days = unix_days_from_ymd(year, month, day)?;
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second;
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs))
}

fn unix_days_from_ymd(year: i64, month: u64, day: u64) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut y = year;
    let mut m = month as i64;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let era = y / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (m - 3) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u64;
    Some(era as u64 * 146097 + doe - 719468)
}

fn file_mtime_secs(path: &str) -> Result<u64, String> {
    let meta = std::fs::metadata(Path::new(path)).map_err(|e| e.to_string())?;
    let modified = meta.modified().map_err(|e| e.to_string())?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_db::Database;

    fn seed_session(conn: &Connection) {
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('sess-auto', 'Auto', 'active')",
            [],
        )
        .unwrap();
    }

    fn sample_trigger(id: &str, trigger_type: TriggerType) -> AutomationTrigger {
        AutomationTrigger {
            trigger_id: id.into(),
            trigger_type,
            session_id: "sess-auto".into(),
            config: TriggerConfig {
                interval_secs: Some(1),
                watch_path: None,
                webhook_secret: Some("secret".into()),
            },
            task_prompt: "plan:[]".into(),
            workspace_path: None,
            enabled: true,
            last_fired_at: None,
        }
    }

    #[test]
    fn cron_tick_writes_audit_without_grant() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        seed_session(&conn);

        let trigger = sample_trigger("trg-cron-deny", TriggerType::Cron);
        AutomationScheduler::register_trigger(&conn, &trigger).unwrap();

        let mut scheduler = AutomationScheduler::new();
        let outcomes = scheduler
            .tick_cron(&conn, SystemTime::now())
            .expect("tick");
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0], AutomationOutcome::Denied { .. }));

        let denied_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE tool_name = 'automation_run' AND decision = 'denied'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(denied_count, 1);
    }

    #[test]
    fn cron_tick_audits_with_grant() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        seed_session(&conn);
        AutomationGrant::grant(&conn, "trg-cron-ok", "sess-auto").unwrap();

        let trigger = sample_trigger("trg-cron-ok", TriggerType::Cron);
        AutomationScheduler::register_trigger(&conn, &trigger).unwrap();

        let mut scheduler = AutomationScheduler::new();
        let outcomes = scheduler
            .tick_cron(&conn, SystemTime::now())
            .expect("tick");
        assert!(matches!(outcomes[0], AutomationOutcome::Audited { .. }));

        let args: String = conn
            .query_row(
                "SELECT arguments_json FROM audit_log WHERE tool_name = 'automation_run' AND decision = 'approved' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(args.contains("trg-cron-ok"));
    }

    #[test]
    fn file_watch_enqueues_on_change() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        seed_session(&conn);
        AutomationGrant::grant(&conn, "trg-file", "sess-auto").unwrap();

        let dir = tempfile::tempdir().unwrap();
        let watch = dir.path().join("watch.txt");
        std::fs::write(&watch, "v1").unwrap();

        let mut trigger = sample_trigger("trg-file", TriggerType::FileWatch);
        trigger.config.watch_path = Some(watch.to_string_lossy().to_string());
        AutomationScheduler::register_trigger(&conn, &trigger).unwrap();

        let mut scheduler = AutomationScheduler::new();
        let baseline = scheduler.poll_file_watchers(&conn).unwrap();
        assert!(baseline.iter().all(|o| matches!(o, AutomationOutcome::Skipped { .. })));

        std::thread::sleep(std::time::Duration::from_secs(1));
        std::fs::write(&watch, "v2").unwrap();
        let fired = scheduler.poll_file_watchers(&conn).unwrap();
        assert!(matches!(fired[0], AutomationOutcome::Enqueued { .. }));

        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automation_queue WHERE trigger_id = 'trg-file' AND status = 'pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);
    }

    #[test]
    fn pr_webhook_denies_bad_secret() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        seed_session(&conn);

        let trigger = sample_trigger("trg-pr", TriggerType::PrWebhook);
        AutomationScheduler::register_trigger(&conn, &trigger).unwrap();
        AutomationGrant::grant(&conn, "trg-pr", "sess-auto").unwrap();

        let mut scheduler = AutomationScheduler::new();
        let outcome = scheduler
            .handle_pr_webhook(&conn, "trg-pr", r#"{"action":"opened"}"#, Some("wrong"))
            .unwrap();
        assert!(matches!(outcome, AutomationOutcome::Denied { .. }));
    }
}
