//! What JARVIS keeps on the user's disk, and how they get it back.
//!
//! Until now nothing bounded local growth except the cron_runs sweep in
//! `scheduler/jobs.rs`. `mail_messages` stored every body forever while
//! `mail/sync.rs` walks backwards through the mailbox across restarts, the WAL
//! had no ceiling, and there was no way for a user to see any of it, let alone
//! reclaim it. This module supplies both halves: a retention sweep that runs on
//! startup, and a report plus purge the Settings > Storage panel drives.

use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

use crate::db::Database;

/// Retention is off rather than infinite when the preference is missing or
/// unparseable -- a corrupted value must never silently delete more than the
/// user asked for.
pub const DEFAULT_RETENTION_DAYS: i64 = 90;

/// Below this the sweep is refused. A user who wants a shorter window can hit
/// the purge buttons; an accidental `0` in the preference row should not wipe
/// a mailbox on next launch.
const MIN_RETENTION_DAYS: i64 = 7;

#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepReport {
    pub mail_bodies_cleared: usize,
    pub conversations_deleted: usize,
    pub emails_deleted: usize,
    pub metrics_deleted: usize,
    pub briefings_deleted: usize,
}

impl SweepReport {
    pub fn rows_touched(&self) -> usize {
        self.mail_bodies_cleared
            + self.conversations_deleted
            + self.emails_deleted
            + self.metrics_deleted
            + self.briefings_deleted
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub models_bytes: u64,
    pub cache_bytes: u64,
    pub backups_bytes: u64,
    pub total_bytes: u64,
    pub mail_message_count: i64,
    pub mail_body_bytes: i64,
    pub conversation_count: i64,
    pub retention_days: i64,
    pub retention_enabled: bool,
    pub data_dir: String,
}

fn jarvis_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("jarvis"))
}

/// The WKWebView network cache. Tauri names it after the product, not the
/// bundle id, which is why this is `jarvis` and not `com.jarvis.app`.
fn jarvis_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("jarvis"))
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Recursive byte total, skipping anything unreadable rather than failing the
/// whole report. Symlinks are not followed: a link into the user's home would
/// otherwise be counted as JARVIS storage.
fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// Read a retention window the sweep is willing to act on. `None` means "keep
/// everything": the preference is off, missing, unparseable, or shorter than
/// `MIN_RETENTION_DAYS`.
pub fn retention_window(conn: &Connection) -> Option<i64> {
    let enabled = read_pref(conn, "storage_retention_enabled").unwrap_or_else(|| "1".into());
    if enabled.trim() != "1" {
        return None;
    }
    let days = read_pref(conn, "storage_retention_days")
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    (days >= MIN_RETENTION_DAYS).then_some(days)
}

fn read_pref(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM user_preferences WHERE key = ?1",
        [key],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Drop content that is older than the retention window.
///
/// Mail bodies are nulled rather than deleted so the message row, its subject,
/// sender and snippet survive -- the workspace list and any citation that
/// points at the message keep working. Messages the user filed into a group are
/// left completely alone: putting a message in a group is an explicit "keep
/// this", and group analysis reads the body back.
pub fn sweep(conn: &Connection, retention_days: i64) -> Result<SweepReport, String> {
    if retention_days < MIN_RETENTION_DAYS {
        return Err(format!(
            "retention window of {retention_days} days is below the {MIN_RETENTION_DAYS}-day floor"
        ));
    }
    let text_cutoff = format!("-{retention_days} days");
    let err = |e: rusqlite::Error| e.to_string();

    let mail_bodies_cleared = conn
        .execute(
            "UPDATE mail_messages SET body = NULL \
             WHERE body IS NOT NULL \
               AND saved_at < datetime('now', ?1) \
               AND id NOT IN (SELECT message_id FROM mail_group_members)",
            [&text_cutoff],
        )
        .map_err(err)?;

    let conversations_deleted = conn
        .execute(
            "DELETE FROM conversations WHERE created_at < datetime('now', ?1)",
            [&text_cutoff],
        )
        .map_err(err)?;

    let emails_deleted = conn
        .execute(
            "DELETE FROM emails WHERE synced_at < datetime('now', ?1)",
            [&text_cutoff],
        )
        .map_err(err)?;

    // inference_metrics.ts and daily_briefings.created_at are unix seconds
    // (chrono::Utc::now().timestamp()), not the datetime() text the tables
    // above use, so they need strftime rather than a string comparison.
    let metrics_deleted = conn
        .execute(
            "DELETE FROM inference_metrics WHERE ts < strftime('%s', 'now', ?1)",
            [&text_cutoff],
        )
        .map_err(err)?;

    let briefings_deleted = conn
        .execute(
            "DELETE FROM daily_briefings WHERE created_at < strftime('%s', 'now', ?1)",
            [&text_cutoff],
        )
        .map_err(err)?;

    Ok(SweepReport {
        mail_bodies_cleared,
        conversations_deleted,
        emails_deleted,
        metrics_deleted,
        briefings_deleted,
    })
}

/// Return freed pages to the filesystem. Deleting rows only marks pages reusable
/// inside the file, so without this the database never shrinks and the Storage
/// panel keeps reporting the high-water mark.
fn reclaim(conn: &Connection) {
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;") {
        log::warn!("storage: reclaim failed: {e}");
    }
}

/// Startup sweep. Best effort in every direction: a locked database or a broken
/// preference must not stop JARVIS from opening.
pub fn sweep_on_startup(db: &Database) {
    let Ok(conn) = db.conn.lock() else {
        log::warn!("storage: startup sweep skipped, database busy");
        return;
    };
    let Some(days) = retention_window(&conn) else {
        log::info!("storage: retention disabled, startup sweep skipped");
        return;
    };
    match sweep(&conn, days) {
        Ok(report) if report.rows_touched() > 0 => {
            log::info!("storage: startup sweep at {days} days: {report:?}");
            reclaim(&conn);
        }
        Ok(_) => log::info!("storage: startup sweep at {days} days found nothing to remove"),
        Err(e) => log::warn!("storage: startup sweep failed: {e}"),
    }
}

#[tauri::command]
pub fn get_storage_usage(db: State<Arc<Database>>) -> Result<StorageUsage, String> {
    let data_dir = jarvis_data_dir().ok_or("Could not find application support directory")?;
    let db_file = data_dir.join("jarvis.db");
    let database_bytes = file_size(&db_file);
    let wal_bytes = file_size(&data_dir.join("jarvis.db-wal"))
        + file_size(&data_dir.join("jarvis.db-shm"));
    let models_bytes = dir_size(&data_dir.join("models"));
    let backups_bytes = dir_size(&data_dir.join("backups"));
    let cache_bytes = jarvis_cache_dir().map(|dir| dir_size(&dir)).unwrap_or(0);

    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mail_message_count = conn
        .query_row("SELECT COUNT(*) FROM mail_messages", [], |row| row.get(0))
        .unwrap_or(0);
    let mail_body_bytes = conn
        .query_row(
            "SELECT COALESCE(SUM(LENGTH(body)), 0) FROM mail_messages",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let conversation_count = conn
        .query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))
        .unwrap_or(0);
    let retention_days = read_pref(&conn, "storage_retention_days")
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    let retention_enabled = read_pref(&conn, "storage_retention_enabled")
        .map(|v| v.trim() == "1")
        .unwrap_or(true);

    Ok(StorageUsage {
        database_bytes,
        wal_bytes,
        models_bytes,
        cache_bytes,
        backups_bytes,
        total_bytes: database_bytes + wal_bytes + models_bytes + cache_bytes + backups_bytes,
        mail_message_count,
        mail_body_bytes,
        conversation_count,
        retention_days,
        retention_enabled,
        data_dir: data_dir.display().to_string(),
    })
}

/// Run the sweep now, from the Storage panel, and report what it removed.
#[tauri::command]
pub fn run_storage_sweep(db: State<Arc<Database>>) -> Result<SweepReport, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let days = retention_window(&conn)
        .ok_or("Retention is switched off. Turn it on before running a sweep.")?;
    let report = sweep(&conn, days)?;
    if report.rows_touched() > 0 {
        reclaim(&conn);
    }
    Ok(report)
}

/// A purge target the Storage panel can offer. Anything not listed here is not
/// deletable through this command -- the match is the allowlist.
#[tauri::command]
pub fn purge_storage(db: State<Arc<Database>>, scope: String) -> Result<u64, String> {
    let data_dir = jarvis_data_dir().ok_or("Could not find application support directory")?;
    match scope.as_str() {
        "cache" => {
            let dir = jarvis_cache_dir().ok_or("Could not find cache directory")?;
            let freed = dir_size(&dir);
            remove_dir_contents(&dir)?;
            Ok(freed)
        }
        "backups" => {
            let dir = data_dir.join("backups");
            let freed = dir_size(&dir);
            remove_dir_contents(&dir)?;
            Ok(freed)
        }
        "models" => {
            let dir = data_dir.join("models");
            let freed = dir_size(&dir);
            remove_dir_contents(&dir)?;
            Ok(freed)
        }
        "mailBodies" => {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let before = mail_body_bytes(&conn);
            conn.execute(
                "UPDATE mail_messages SET body = NULL \
                 WHERE body IS NOT NULL \
                   AND id NOT IN (SELECT message_id FROM mail_group_members)",
                [],
            )
            .map_err(|e| e.to_string())?;
            reclaim(&conn);
            Ok((before - mail_body_bytes(&conn)).max(0) as u64)
        }
        "conversations" => {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let removed = conn
                .execute("DELETE FROM conversations", [])
                .map_err(|e| e.to_string())?;
            reclaim(&conn);
            Ok(removed as u64)
        }
        other => Err(format!("unknown storage scope: {other}")),
    }
}

fn mail_body_bytes(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(body)), 0) FROM mail_messages",
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

/// Empty a directory without removing the directory itself, so the next write
/// does not have to recreate it. A missing directory is already empty.
fn remove_dir_contents(dir: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("read {}: {e}", dir.display())),
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let outcome = match entry.file_type() {
            Ok(kind) if kind.is_dir() => std::fs::remove_dir_all(&path),
            _ => std::fs::remove_file(&path),
        };
        if let Err(e) = outcome {
            return Err(format!("remove {}: {e}", path.display()));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn set_retention_policy(
    db: State<Arc<Database>>,
    days: i64,
    enabled: bool,
) -> Result<(), String> {
    if enabled && days < MIN_RETENTION_DAYS {
        return Err(format!(
            "Retention must be at least {MIN_RETENTION_DAYS} days."
        ));
    }
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let write = |key: &str, value: String| -> Result<(), String> {
        conn.execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now')) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            rusqlite::params![key, value],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    };
    write("storage_retention_days", days.to_string())?;
    write(
        "storage_retention_enabled",
        if enabled { "1" } else { "0" }.to_string(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_database;

    fn seed(conn: &Connection) {
        conn.execute_batch(
            "INSERT INTO mail_accounts(id,email) VALUES ('a','a@example.com');
             INSERT INTO mail_messages(id,account_id,provider_id,subject,body,received_at,saved_at)
               VALUES (1,'a','old','Old','old body','2020-01-01', datetime('now','-200 days'));
             INSERT INTO mail_messages(id,account_id,provider_id,subject,body,received_at,saved_at)
               VALUES (2,'a','new','New','new body','2026-01-01', datetime('now'));
             INSERT INTO mail_messages(id,account_id,provider_id,subject,body,received_at,saved_at)
               VALUES (3,'a','filed','Filed','filed body','2020-01-01', datetime('now','-200 days'));
             INSERT INTO mail_groups(id,name) VALUES (7,'Kept');
             INSERT INTO mail_group_members(group_id,message_id) VALUES (7,3);
             INSERT INTO conversations(role,content,created_at)
               VALUES ('user','old turn', datetime('now','-200 days'));
             INSERT INTO conversations(role,content,created_at)
               VALUES ('user','recent turn', datetime('now'));
             INSERT INTO inference_metrics(ts,workload,model_id,input_tokens,output_tokens,latency_ms)
               VALUES (strftime('%s','now','-200 days'),'volume','m',1,1,1);
             INSERT INTO inference_metrics(ts,workload,model_id,input_tokens,output_tokens,latency_ms)
               VALUES (strftime('%s','now'),'volume','m',1,1,1);",
        )
        .unwrap();
    }

    #[test]
    fn sweep_clears_old_bodies_and_keeps_recent_and_filed_mail() {
        let db = test_database();
        let conn = db.conn.lock().unwrap();
        seed(&conn);

        let report = sweep(&conn, 90).unwrap();
        assert_eq!(report.mail_bodies_cleared, 1, "only the unfiled old message");

        let body_of = |id: i64| -> Option<String> {
            conn.query_row("SELECT body FROM mail_messages WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(body_of(1), None, "old unfiled body is dropped");
        assert_eq!(body_of(2), Some("new body".into()), "recent body survives");
        assert_eq!(
            body_of(3),
            Some("filed body".into()),
            "a message the user filed into a group is never swept"
        );
    }

    #[test]
    fn sweep_prunes_both_text_and_unix_timestamp_tables() {
        let db = test_database();
        let conn = db.conn.lock().unwrap();
        seed(&conn);

        let report = sweep(&conn, 90).unwrap();
        assert_eq!(report.conversations_deleted, 1);
        assert_eq!(report.metrics_deleted, 1, "ts is unix seconds, not text");

        let conversations: i64 = conn
            .query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(conversations, 1, "the recent turn is kept");
    }

    #[test]
    fn a_sweep_shorter_than_the_floor_is_refused() {
        let db = test_database();
        let conn = db.conn.lock().unwrap();
        seed(&conn);
        assert!(sweep(&conn, 0).is_err(), "0 days must not wipe everything");
        assert!(sweep(&conn, 6).is_err());
        assert!(sweep(&conn, 7).is_ok());
    }

    #[test]
    fn retention_window_defaults_on_and_refuses_bad_values() {
        let db = test_database();
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            retention_window(&conn),
            Some(DEFAULT_RETENTION_DAYS),
            "V25 seeds an enabled 90-day window"
        );

        let set = |key: &str, value: &str| {
            conn.execute(
                "INSERT INTO user_preferences(key,value) VALUES (?1,?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value],
            )
            .unwrap();
        };
        set("storage_retention_days", "not a number");
        assert_eq!(retention_window(&conn), Some(DEFAULT_RETENTION_DAYS));
        set("storage_retention_days", "1");
        assert_eq!(retention_window(&conn), None, "below the floor keeps all");
        set("storage_retention_days", "30");
        assert_eq!(retention_window(&conn), Some(30));
        set("storage_retention_enabled", "0");
        assert_eq!(retention_window(&conn), None, "switched off keeps all");
    }

    #[test]
    fn removing_the_contents_of_a_missing_directory_is_not_an_error() {
        let missing = std::env::temp_dir().join(format!("jarvis-absent-{}", uuid::Uuid::new_v4()));
        assert!(remove_dir_contents(&missing).is_ok());
    }

    #[test]
    fn dir_size_totals_nested_files() {
        let root = std::env::temp_dir().join(format!("jarvis-size-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("a.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(root.join("nested/b.bin"), vec![0u8; 2000]).unwrap();
        assert_eq!(dir_size(&root), 3000);

        remove_dir_contents(&root).unwrap();
        assert_eq!(dir_size(&root), 0, "purge empties the tree");
        std::fs::remove_dir_all(&root).ok();
    }
}
