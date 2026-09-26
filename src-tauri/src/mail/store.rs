use super::models::*;
use crate::{db::Database, integrations::gmail::GmailMessage};
use rusqlite::{params, Connection, OptionalExtension, Row};

fn err(e: impl std::fmt::Display) -> String {
    format!("本機電郵資料操作失敗：{e}")
}
fn required(changed: usize) -> Result<(), String> {
    if changed == 0 {
        Err("找不到指定資料，請重新整理。".into())
    } else {
        Ok(())
    }
}
fn text(value: &str, max: usize, allow_empty: bool) -> Result<&str, String> {
    let value = value.trim();
    if (!allow_empty && value.is_empty()) || value.chars().count() > max {
        return Err(format!("內容不可留空或超過 {max} 字。"));
    }
    Ok(value)
}
const ACCOUNT_SELECT: &str = "SELECT a.id,a.email,a.connected,a.last_synced,a.next_page_token,(SELECT COUNT(*) FROM mail_messages m WHERE m.account_id=a.id) FROM mail_accounts a";
fn account_row(r: &Row<'_>) -> rusqlite::Result<MailAccount> {
    Ok(MailAccount {
        id: r.get(0)?,
        email: r.get(1)?,
        connected: r.get(2)?,
        last_synced: r.get(3)?,
        next_page_token: r.get(4)?,
        message_count: r.get(5)?,
    })
}
pub fn get_account(db: &Database, id: &str) -> Result<MailAccount, String> {
    db.conn
        .lock()
        .map_err(err)?
        .query_row(
            &format!("{ACCOUNT_SELECT} WHERE a.id=?1"),
            [id],
            account_row,
        )
        .map_err(err)
}
pub fn list_accounts(db: &Database) -> Result<Vec<MailAccount>, String> {
    let conn = db.conn.lock().map_err(err)?;
    let mut stmt = conn
        .prepare(&format!("{ACCOUNT_SELECT} ORDER BY a.email"))
        .map_err(err)?;
    let rows = stmt
        .query_map([], account_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}
pub fn find_account_by_email(db: &Database, email: &str) -> Result<Option<MailAccount>, String> {
    db.conn
        .lock()
        .map_err(err)?
        .query_row(
            &format!("{ACCOUNT_SELECT} WHERE a.email=?1"),
            [email],
            account_row,
        )
        .optional()
        .map_err(err)
}
pub fn upsert_account(
    db: &Database,
    id: &str,
    email: &str,
    connected: bool,
) -> Result<MailAccount, String> {
    let email = text(email, 320, false)?;
    db.conn.lock().map_err(err)?.execute("INSERT INTO mail_accounts(id,email,connected) VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET email=excluded.email,connected=excluded.connected",params![id,email,connected]).map_err(err)?;
    get_account(db, id)
}
pub fn set_connected(db: &Database, id: &str, connected: bool) -> Result<(), String> {
    required(
        db.conn
            .lock()
            .map_err(err)?
            .execute(
                "UPDATE mail_accounts SET connected=?2 WHERE id=?1",
                params![id, connected],
            )
            .map_err(err)?,
    )
}
pub fn delete_account(db: &Database, id: &str) -> Result<(), String> {
    let mut conn = db.conn.lock().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    // Explicit local deletion also removes notes sourced from that account.
    tx.execute("DELETE FROM mail_memories WHERE source_message_id IN (SELECT id FROM mail_messages WHERE account_id=?1)",[id]).map_err(err)?;
    required(
        tx.execute("DELETE FROM mail_accounts WHERE id=?1", [id])
            .map_err(err)?,
    )?;
    tx.commit().map_err(err)
}
/// `touch_last_synced` must be true only when everything since the previous
/// last_synced is stored: the next normal sync lists mail from there, so
/// moving it early (after "load older", say) silently skips new mail.
pub fn finish_sync(
    db: &Database,
    id: &str,
    next_page_token: Option<&str>,
    advance_cursor: bool,
    touch_last_synced: bool,
) -> Result<(), String> {
    required(db.conn.lock().map_err(err)?.execute("UPDATE mail_accounts SET last_synced=CASE WHEN ?4 THEN datetime('now') ELSE last_synced END,next_page_token=CASE WHEN ?3 THEN ?2 ELSE next_page_token END WHERE id=?1",params![id,next_page_token,advance_cursor,touch_last_synced]).map_err(err)?)
}

/// Refresh Gmail's read and inbox state for a message already stored.
pub fn update_labels(db: &Database, account_id: &str, provider_id: &str, is_read: bool, is_inbox: bool) -> Result<(), String> {
    db.conn.lock().map_err(err)?
        .execute("UPDATE mail_messages SET is_read=?3,is_inbox=?4 WHERE account_id=?1 AND provider_id=?2", params![account_id, provider_id, is_read, is_inbox])
        .map_err(err)?;
    Ok(())
}

/// Gmail's history cursor for this account, if a complete sync has set one.
pub fn history_id(db: &Database, account_id: &str) -> Result<Option<String>, String> {
    db.conn.lock().map_err(err)?
        .query_row("SELECT history_id FROM mail_accounts WHERE id=?1", [account_id], |r| r.get(0))
        .map_err(err)
}
/// Set only once everything up to `history_id` is stored; None drops an
/// expired cursor so the next sync falls back to the date window.
pub fn set_history_id(db: &Database, account_id: &str, history_id: Option<&str>) -> Result<(), String> {
    required(db.conn.lock().map_err(err)?
        .execute("UPDATE mail_accounts SET history_id=?2 WHERE id=?1", params![account_id, history_id])
        .map_err(err)?)
}
/// Seconds since a complete window first failed to download a message,
/// starting the clock now if this is the first such sync.
pub fn retrying_for(db: &Database, account_id: &str) -> Result<i64, String> {
    let conn = db.conn.lock().map_err(err)?;
    conn.execute("UPDATE mail_accounts SET retry_since=COALESCE(retry_since,datetime('now')) WHERE id=?1", [account_id]).map_err(err)?;
    conn.query_row("SELECT CAST(strftime('%s','now') AS INTEGER)-CAST(strftime('%s',retry_since) AS INTEGER) FROM mail_accounts WHERE id=?1", [account_id], |r| r.get(0))
        .map_err(err)
}
pub fn clear_retry(db: &Database, account_id: &str) -> Result<(), String> {
    db.conn.lock().map_err(err)?
        .execute("UPDATE mail_accounts SET retry_since=NULL WHERE id=?1", [account_id])
        .map_err(err)?;
    Ok(())
}
/// Apply read and inbox changes reported by Gmail history. None leaves that
/// flag as stored.
pub fn apply_label_changes(db: &Database, account_id: &str, changes: &[(String, Option<bool>, Option<bool>)]) -> Result<(), String> {
    let mut conn = db.conn.lock().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    for (provider_id, is_read, is_inbox) in changes {
        tx.execute("UPDATE mail_messages SET is_read=COALESCE(?3,is_read),is_inbox=COALESCE(?4,is_inbox) WHERE account_id=?1 AND provider_id=?2",
            params![account_id, provider_id, is_read, is_inbox]).map_err(err)?;
    }
    tx.commit().map_err(err)
}

/// The user's "handled" / "needs attention" override for one message.
pub fn set_attention(db: &Database, message_id: i64, attention: bool) -> Result<(), String> {
    required(db.conn.lock().map_err(err)?
        .execute("UPDATE mail_triage SET attention=?2 WHERE message_id=?1", params![message_id, attention])
        .map_err(err)?)
}
pub fn save_messages(
    db: &Database,
    account_id: &str,
    rows: &[(GmailMessage, String)],
) -> Result<usize, String> {
    let mut conn = db.conn.lock().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    for (m, body) in rows {
        let attachments = serde_json::to_string(&m.attachments).map_err(err)?;
        tx.execute("INSERT INTO mail_messages(account_id,provider_id,thread_id,subject,sender,snippet,body,is_read,is_inbox,received_at,attachments)
          VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
          ON CONFLICT(account_id,provider_id) DO UPDATE SET thread_id=excluded.thread_id,subject=excluded.subject,sender=excluded.sender,snippet=excluded.snippet,body=excluded.body,is_read=excluded.is_read,is_inbox=excluded.is_inbox,received_at=excluded.received_at,attachments=excluded.attachments,saved_at=datetime('now')",
          params![account_id,m.id,m.thread_id,m.subject.as_deref().unwrap_or(""),m.sender.as_deref().unwrap_or(""),m.snippet.as_deref().unwrap_or(""),body,m.is_read,m.label_ids.iter().any(|l|l=="INBOX"),m.received_at.as_deref().unwrap_or(""),attachments]).map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(rows.len())
}
const MESSAGE_COLUMNS: &str =
    "m.id,m.account_id,a.email,m.provider_id,m.thread_id,m.subject,m.sender,m.snippet,m.attachments";
/// Appended after `m.received_at` in every message query; message_row reads
/// them from index 13. The last column is the earliest open follow-up date.
const TRIAGE_COLUMNS: &str = ",t.category,t.category_confidence,t.needs_reply,t.action_required,t.has_date,t.importance,t.sent_by_owner,t.attention,t.summary,t.decided_by,(SELECT MIN(i.due_date) FROM mail_items i WHERE i.message_id=m.id AND i.status='open' AND i.due_date IS NOT NULL)";
const TRIAGE_JOIN: &str = " LEFT JOIN mail_triage t ON t.message_id=m.id";
fn triage_from_row(r: &Row<'_>) -> rusqlite::Result<Option<MailTriage>> {
    let Some(category) = r.get::<_, Option<String>>(13)? else { return Ok(None) };
    Ok(Some(MailTriage {
        category,
        category_confidence: r.get(14)?,
        needs_reply: r.get(15)?,
        action_required: r.get(16)?,
        has_date: r.get(17)?,
        importance: r.get(18)?,
        sent_by_owner: r.get(19)?,
        attention: r.get(20)?,
        summary: r.get(21)?,
        decided_by: r.get(22)?,
    }))
}
fn message_row(r: &Row<'_>) -> rusqlite::Result<MailMessage> {
    Ok(MailMessage {
        triage: triage_from_row(r)?,
        next_due: r.get(23)?,
        id: r.get(0)?,
        account_id: r.get(1)?,
        account_email: r.get(2)?,
        provider_id: r.get(3)?,
        thread_id: r.get(4)?,
        subject: r.get(5)?,
        sender: r.get(6)?,
        snippet: r.get(7)?,
        attachments: r.get::<_, Option<String>>(8)?.map(|json| serde_json::from_str(&json).unwrap_or_default()),
        body: r.get(9)?,
        is_read: r.get(10)?,
        is_inbox: r.get(11)?,
        received_at: r.get(12)?,
    })
}
pub fn get_message(db: &Database, id: i64) -> Result<MailMessage, String> {
    db.conn.lock().map_err(err)?.query_row(&format!("SELECT {MESSAGE_COLUMNS},m.body,m.is_read,m.is_inbox,m.received_at{TRIAGE_COLUMNS} FROM mail_messages m JOIN mail_accounts a ON a.id=m.account_id{TRIAGE_JOIN} WHERE m.id=?1"),[id],message_row).map_err(err)
}
/// `view` narrows the list to a smart view: "attention", "due", "untriaged"
/// or one of MAIL_CATEGORIES. Anything else is rejected.
pub fn list_messages(
    db: &Database,
    account_id: Option<&str>,
    group_id: Option<i64>,
    query: &str,
    offset: u32,
    view: Option<&str>,
) -> Result<MailPage, String> {
    if let Some(view) = view {
        if !["attention", "due", "untriaged"].contains(&view) && !MAIL_CATEGORIES.contains(&view) {
            return Err("未知的郵件分類。".into());
        }
    }
    let query = text(query, 500, true)?;
    let pattern = format!(
        "%{}%",
        query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let filter=format!("FROM mail_messages m JOIN mail_accounts a ON a.id=m.account_id{TRIAGE_JOIN} WHERE (?1 IS NULL OR m.account_id=?1) AND (?2 IS NULL OR EXISTS(SELECT 1 FROM mail_group_members gm WHERE gm.group_id=?2 AND gm.message_id=m.id)) AND (?3='%%' OR m.subject LIKE ?3 ESCAPE '\\' OR m.sender LIKE ?3 ESCAPE '\\' OR m.body LIKE ?3 ESCAPE '\\' OR m.snippet LIKE ?3 ESCAPE '\\' OR t.summary LIKE ?3 ESCAPE '\\') AND (?5 IS NULL OR (?5='attention' AND t.attention=1) OR (?5='untriaged' AND t.message_id IS NULL) OR (?5='due' AND EXISTS(SELECT 1 FROM mail_items i WHERE i.message_id=m.id AND i.status='open')) OR t.category=?5)");
    let conn = db.conn.lock().map_err(err)?;
    // SQLite counts parameters up to the highest index (?5), so the count
    // statement takes all five even though it never reads the offset.
    let total = conn
        .query_row(
            &format!("SELECT COUNT(*) {filter}"),
            params![account_id, group_id, pattern, offset, view],
            |r| r.get(0),
        )
        .map_err(err)?;
    let mut stmt=conn.prepare(&format!("SELECT {MESSAGE_COLUMNS},NULL,m.is_read,m.is_inbox,m.received_at{TRIAGE_COLUMNS} {filter} ORDER BY m.received_at DESC,m.id DESC LIMIT 50 OFFSET ?4")).map_err(err)?;
    let messages = stmt
        .query_map(params![account_id, group_id, pattern, offset, view], message_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(MailPage { messages, total })
}
const GROUP_SELECT: &str="SELECT g.id,g.name,g.instructions,(SELECT COUNT(*) FROM mail_group_members gm WHERE gm.group_id=g.id) FROM mail_groups g";
fn group_row(r: &Row<'_>) -> rusqlite::Result<MailGroup> {
    Ok(MailGroup {
        id: r.get(0)?,
        name: r.get(1)?,
        instructions: r.get(2)?,
        message_count: r.get(3)?,
    })
}
fn get_group_conn(conn: &Connection, id: i64) -> Result<MailGroup, String> {
    conn.query_row(&format!("{GROUP_SELECT} WHERE g.id=?1"), [id], group_row)
        .map_err(err)
}
pub fn list_groups(db: &Database) -> Result<Vec<MailGroup>, String> {
    let conn = db.conn.lock().map_err(err)?;
    let mut stmt = conn
        .prepare(&format!("{GROUP_SELECT} ORDER BY g.name"))
        .map_err(err)?;
    let rows = stmt
        .query_map([], group_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}
pub fn save_group(
    db: &Database,
    id: Option<i64>,
    name: &str,
    instructions: &str,
) -> Result<MailGroup, String> {
    let name = text(name, 100, false)?;
    let instructions = text(instructions, 8000, true)?;
    let conn = db.conn.lock().map_err(err)?;
    let id = if let Some(id) = id {
        required(conn.execute("UPDATE mail_groups SET name=?2,instructions=?3,updated_at=datetime('now') WHERE id=?1",params![id,name,instructions]).map_err(err)?)?;
        id
    } else {
        conn.execute(
            "INSERT INTO mail_groups(name,instructions) VALUES (?1,?2)",
            params![name, instructions],
        )
        .map_err(err)?;
        conn.last_insert_rowid()
    };
    get_group_conn(&conn, id)
}
pub fn delete_group(db: &Database, id: i64) -> Result<(), String> {
    required(
        db.conn
            .lock()
            .map_err(err)?
            .execute("DELETE FROM mail_groups WHERE id=?1", [id])
            .map_err(err)?,
    )
}
pub fn set_group_members(
    db: &Database,
    group_id: i64,
    message_ids: &[i64],
    add: bool,
) -> Result<(), String> {
    if message_ids.is_empty() || message_ids.len() > 500 {
        return Err("請選擇 1 至 500 封郵件。".into());
    }
    let mut conn = db.conn.lock().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    get_group_conn(&tx, group_id)?;
    for id in message_ids {
        if add {
            tx.execute("INSERT INTO mail_group_members(group_id,message_id) VALUES (?1,?2) ON CONFLICT(group_id,message_id) DO NOTHING",params![group_id,id]).map_err(err)?;
        } else {
            tx.execute(
                "DELETE FROM mail_group_members WHERE group_id=?1 AND message_id=?2",
                params![group_id, id],
            )
            .map_err(err)?;
        }
    }
    tx.commit().map_err(err)
}
const MEMORY_SELECT:&str="SELECT id,group_id,content,kind,source_message_id,expires_at,status,updated_at FROM mail_memories";
fn memory_row(r: &Row<'_>) -> rusqlite::Result<MailMemory> {
    Ok(MailMemory {
        id: r.get(0)?,
        group_id: r.get(1)?,
        content: r.get(2)?,
        kind: r.get(3)?,
        source_message_id: r.get(4)?,
        expires_at: r.get(5)?,
        status: r.get(6)?,
        updated_at: r.get(7)?,
    })
}
pub fn list_memories(db: &Database, group_id: i64) -> Result<Vec<MailMemory>, String> {
    let conn = db.conn.lock().map_err(err)?;
    get_group_conn(&conn, group_id)?;
    let mut stmt = conn
        .prepare(&format!(
            "{MEMORY_SELECT} WHERE group_id=?1 ORDER BY id DESC"
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map([group_id], memory_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}
pub fn save_memory(db: &Database, input: &MailMemoryInput) -> Result<MailMemory, String> {
    let content = text(&input.content, 2000, false)?;
    if !["fact", "preference", "commitment"].contains(&input.kind.as_str())
        || !["active", "done"].contains(&input.status.as_str())
    {
        return Err("記憶種類或狀態不正確。".into());
    }
    let expires = input.expires_at.as_deref().filter(|s| !s.is_empty());
    if let Some(date) = expires {
        if date.len() != 10
            || chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map(|d| d.to_string() != date)
                .unwrap_or(true)
        {
            return Err("到期日請使用 YYYY-MM-DD 格式。".into());
        }
    }
    let conn = db.conn.lock().map_err(err)?;
    get_group_conn(&conn, input.group_id)?;
    if let Some(source) = input.source_message_id {
        let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM mail_group_members WHERE group_id=?1 AND message_id=?2)",params![input.group_id,source],|r|r.get(0)).map_err(err)?;
        if !valid {
            return Err("來源郵件必須屬於此群組。".into());
        }
    }
    let id = if let Some(id) = input.id {
        required(conn.execute("UPDATE mail_memories SET content=?3,kind=?4,source_message_id=?5,expires_at=?6,status=?7,updated_at=datetime('now') WHERE id=?1 AND group_id=?2",params![id,input.group_id,content,input.kind,input.source_message_id,expires,input.status]).map_err(err)?)?;
        id
    } else {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_memories WHERE group_id=?1",
                [input.group_id],
                |r| r.get(0),
            )
            .map_err(err)?;
        if count >= 200 {
            return Err("每組最多儲存 200 條記憶，請先整理。".into());
        }
        conn.execute("INSERT INTO mail_memories(group_id,content,kind,source_message_id,expires_at,status) VALUES (?1,?2,?3,?4,?5,?6)",params![input.group_id,content,input.kind,input.source_message_id,expires,input.status]).map_err(err)?;
        conn.last_insert_rowid()
    };
    conn.query_row(&format!("{MEMORY_SELECT} WHERE id=?1"), [id], memory_row)
        .map_err(err)
}
pub fn delete_memory(db: &Database, id: i64) -> Result<(), String> {
    required(
        db.conn
            .lock()
            .map_err(err)?
            .execute("DELETE FROM mail_memories WHERE id=?1", [id])
            .map_err(err)?,
    )
}
fn settings_conn(conn: &Connection) -> Result<MailSettings, String> {
    conn.query_row(
        "SELECT processing_mode,local_model,bedrock_model,openai_model FROM mail_settings WHERE id=1",
        [],
        |r| {
            Ok(MailSettings {
                processing_mode: r.get(0)?,
                local_model: r.get(1)?,
                bedrock_model: r.get(2)?,
                openai_model: r.get(3)?,
            })
        },
    )
    .map_err(err)
}
pub fn settings(db: &Database) -> Result<MailSettings, String> {
    let conn = db.conn.lock().map_err(err)?;
    settings_conn(&conn)
}
pub fn save_settings(db: &Database, input: &MailSettings) -> Result<(), String> {
    let (local, bedrock, openai) = validated_settings(input)?;
    db.conn.lock().map_err(err)?.execute("UPDATE mail_settings SET processing_mode=?1,local_model=?2,bedrock_model=?3,openai_model=?4 WHERE id=1",params![input.processing_mode,local,bedrock,openai]).map_err(err)?;
    Ok(())
}

fn validated_settings(input: &MailSettings) -> Result<(&str, &str, &str), String> {
    if !["local", "bedrock", "openai"].contains(&input.processing_mode.as_str()) {
        return Err("請選擇本機、AWS Bedrock 或 OpenAI。".into());
    }
    let local = text(&input.local_model, 200, true)?;
    let bedrock = text(&input.bedrock_model, 256, true)?;
    let openai = text(&input.openai_model, 200, true)?;
    Ok((local, bedrock, openai))
}

pub fn save_settings_if_unchanged(db: &Database, input: &MailSettings, expected: &MailSettings) -> Result<(), String> {
    let (local, bedrock, openai) = validated_settings(input)?;
    let changed = db.conn.lock().map_err(err)?.execute(
        "UPDATE mail_settings SET processing_mode=?1,local_model=?2,bedrock_model=?3,openai_model=?4 WHERE id=1 AND processing_mode=?5 AND local_model=?6 AND bedrock_model=?7 AND openai_model=?8",
        params![input.processing_mode,local,bedrock,openai,expected.processing_mode,expected.local_model,expected.bedrock_model,expected.openai_model],
    ).map_err(err)?;
    if changed == 1 { Ok(()) } else { Err("settings_changed".into()) }
}
pub fn context(db: &Database, group_id: i64) -> Result<MailContext, String> {
    let conn = db.conn.lock().map_err(err)?;
    let group = get_group_conn(&conn, group_id)?;
    let mut stmt=conn.prepare(&format!("SELECT {MESSAGE_COLUMNS},m.body,m.is_read,m.is_inbox,m.received_at{TRIAGE_COLUMNS} FROM mail_messages m JOIN mail_accounts a ON a.id=m.account_id{TRIAGE_JOIN} JOIN mail_group_members gm ON gm.message_id=m.id WHERE gm.group_id=?1 ORDER BY m.received_at DESC,m.id DESC LIMIT 100")).map_err(err)?;
    let messages = stmt
        .query_map([group_id], message_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut stmt=conn.prepare(&format!("{MEMORY_SELECT} WHERE group_id=?1 AND status='active' AND (expires_at IS NULL OR expires_at>=?2) AND (source_message_id IS NULL OR EXISTS(SELECT 1 FROM mail_group_members gm WHERE gm.group_id=mail_memories.group_id AND gm.message_id=mail_memories.source_message_id)) ORDER BY id DESC")).map_err(err)?;
    let today = chrono::Local::now().date_naive().to_string();
    let memories = stmt
        .query_map(params![group_id, today], memory_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(MailContext {
        group,
        messages,
        memories,
        settings: settings_conn(&conn)?,
    })
}

/// What triage reads for one message. Body is the stored readable text.
#[derive(Debug, Clone)]
pub struct TriageInput {
    pub id: i64,
    pub thread_id: Option<String>,
    pub sender: String,
    pub subject: String,
    pub snippet: String,
    pub body: Option<String>,
    pub received_at: String,
}

/// A follow-up the extractor proposed, before it has an id.
#[derive(Debug, Clone, PartialEq)]
pub struct NewItem {
    pub kind: String,
    pub title: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub end_time: Option<String>,
    pub location: Option<String>,
    pub evidence: String,
    pub evidence_verified: bool,
}

/// Newest untriaged messages first, so a capped run covers what the user is
/// most likely to be looking at.
pub fn untriaged_messages(db: &Database, limit: usize) -> Result<Vec<TriageInput>, String> {
    let conn = db.conn.lock().map_err(err)?;
    let mut stmt = conn.prepare("SELECT m.id,m.thread_id,m.sender,m.subject,m.snippet,m.body,m.received_at FROM mail_messages m LEFT JOIN mail_triage t ON t.message_id=m.id WHERE t.message_id IS NULL ORDER BY m.received_at DESC,m.id DESC LIMIT ?1").map_err(err)?;
    let rows = stmt
        .query_map([limit as i64], |r| {
            Ok(TriageInput {
                id: r.get(0)?,
                thread_id: r.get(1)?,
                sender: r.get(2)?,
                subject: r.get(3)?,
                snippet: r.get(4)?,
                body: r.get(5)?,
                received_at: r.get(6)?,
            })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

pub fn untriaged_count(db: &Database) -> Result<i64, String> {
    db.conn.lock().map_err(err)?
        .query_row("SELECT COUNT(*) FROM mail_messages m LEFT JOIN mail_triage t ON t.message_id=m.id WHERE t.message_id IS NULL", [], |r| r.get(0))
        .map_err(err)
}

/// Senders of messages that arrived later in the same thread. Used to tell
/// whether the owner has already replied.
pub fn later_senders_in_thread(db: &Database, thread_id: &str, received_at: &str) -> Result<Vec<String>, String> {
    let conn = db.conn.lock().map_err(err)?;
    let mut stmt = conn.prepare("SELECT sender FROM mail_messages WHERE thread_id=?1 AND received_at>?2").map_err(err)?;
    let rows = stmt
        .query_map(params![thread_id, received_at], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<Vec<String>, _>>()
        .map_err(err)?;
    Ok(rows)
}

/// Every connected or disconnected mailbox address, lower-cased. Mail from
/// one of these was written by the user, so it never "needs a reply".
pub fn owner_addresses(db: &Database) -> Result<Vec<String>, String> {
    let conn = db.conn.lock().map_err(err)?;
    let mut stmt = conn.prepare("SELECT lower(email) FROM mail_accounts").map_err(err)?;
    let rows = stmt
        .query_map([], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<Vec<String>, _>>()
        .map_err(err)?;
    Ok(rows)
}

/// The domain of a From header's address, lower-cased, for duplicate checks.
fn sender_domain(sender: &str) -> String {
    let address = sender
        .rsplit_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(address, _)| address)
        .unwrap_or(sender);
    address.rsplit_once('@').map(|(_, domain)| domain.trim().trim_matches('"').to_lowercase()).unwrap_or_default()
}

pub fn save_triage(
    db: &Database,
    message_id: i64,
    sender: &str,
    triage: &MailTriage,
    jev_model: &str,
    luna_model: Option<&str>,
    items: &[NewItem],
) -> Result<(), String> {
    let mut conn = db.conn.lock().map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    tx.execute("INSERT INTO mail_triage(message_id,category,category_confidence,needs_reply,action_required,has_date,importance,sent_by_owner,attention,summary,decided_by,jev_model,luna_model) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
      ON CONFLICT(message_id) DO UPDATE SET category=excluded.category,category_confidence=excluded.category_confidence,needs_reply=excluded.needs_reply,action_required=excluded.action_required,has_date=excluded.has_date,importance=excluded.importance,sent_by_owner=excluded.sent_by_owner,attention=excluded.attention,summary=excluded.summary,decided_by=excluded.decided_by,jev_model=excluded.jev_model,luna_model=excluded.luna_model,triaged_at=datetime('now')",
      params![message_id,triage.category,triage.category_confidence,triage.needs_reply,triage.action_required,triage.has_date,triage.importance,triage.sent_by_owner,triage.attention,triage.summary,triage.decided_by,jev_model,luna_model]).map_err(err)?;
    // A re-triage replaces untouched suggestions but keeps anything the user
    // has acted on (done, dismissed, reminded or added to the calendar).
    tx.execute("DELETE FROM mail_items WHERE message_id=?1 AND status='open' AND remind_at IS NULL AND calendar_event_id IS NULL", [message_id]).map_err(err)?;
    let domain = sender_domain(sender);
    for item in items {
        // A booking and its confirmation arrive as two emails from the same
        // sender for the same moment; keep one follow-up. Untimed items only
        // merge when kind and title also match, so two different things due
        // the same day from one organisation both survive.
        if let (Some(due_date), false) = (item.due_date.as_deref(), domain.is_empty()) {
            let mut stmt = tx.prepare("SELECT i.kind,i.title,i.due_time,m.sender FROM mail_items i JOIN mail_messages m ON m.id=i.message_id WHERE i.message_id!=?1 AND i.due_date=?2 AND i.status!='dismissed'").map_err(err)?;
            let candidates = stmt
                .query_map(params![message_id, due_date], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?)))
                .map_err(err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err)?;
            let duplicate = candidates.iter().any(|(kind, title, due_time, other)| {
                sender_domain(other) == domain
                    && match (&item.due_time, due_time) {
                        (Some(a), Some(b)) => a == b,
                        (None, None) => kind == &item.kind && title.trim() == item.title.trim(),
                        _ => false,
                    }
            });
            if duplicate {
                continue;
            }
        }
        tx.execute("INSERT INTO mail_items(message_id,kind,title,due_date,due_time,end_time,location,evidence,evidence_verified) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
          params![message_id,item.kind,item.title,item.due_date,item.due_time,item.end_time,item.location,item.evidence,item.evidence_verified]).map_err(err)?;
    }
    tx.commit().map_err(err)
}

pub fn auto_triage_enabled(db: &Database) -> Result<bool, String> {
    db.conn.lock().map_err(err)?
        .query_row("SELECT auto_triage FROM mail_settings WHERE id=1", [], |r| r.get(0))
        .map_err(err)
}

pub fn set_auto_triage(db: &Database, enabled: bool) -> Result<(), String> {
    required(db.conn.lock().map_err(err)?
        .execute("UPDATE mail_settings SET auto_triage=?1 WHERE id=1", [enabled])
        .map_err(err)?)
}

/// Counts for the sidebar. With `account_id`, they match what the message
/// list shows for that mailbox; without it, every mailbox.
pub fn triage_status(db: &Database, account_id: Option<&str>, typesafe_key: bool, openai_key: bool) -> Result<MailTriageStatus, String> {
    let conn = db.conn.lock().map_err(err)?;
    let count = |sql: &str| -> Result<i64, String> { conn.query_row(sql, [account_id], |r| r.get(0)).map_err(err) };
    let enabled: bool = conn.query_row("SELECT auto_triage FROM mail_settings WHERE id=1", [], |r| r.get(0)).map_err(err)?;
    let untriaged = count("SELECT COUNT(*) FROM mail_messages m LEFT JOIN mail_triage t ON t.message_id=m.id WHERE t.message_id IS NULL AND (?1 IS NULL OR m.account_id=?1)")?;
    let attention = count("SELECT COUNT(*) FROM mail_triage t JOIN mail_messages m ON m.id=t.message_id WHERE t.attention=1 AND (?1 IS NULL OR m.account_id=?1)")?;
    let open_items = count("SELECT COUNT(*) FROM mail_items i JOIN mail_messages m ON m.id=i.message_id WHERE i.status='open' AND (?1 IS NULL OR m.account_id=?1)")?;
    let mut categories = Vec::with_capacity(MAIL_CATEGORIES.len());
    for category in MAIL_CATEGORIES {
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM mail_triage t JOIN mail_messages m ON m.id=t.message_id WHERE t.category=?1 AND (?2 IS NULL OR m.account_id=?2)", params![category, account_id], |r| r.get(0)).map_err(err)?;
        categories.push((category.to_string(), n));
    }
    Ok(MailTriageStatus { enabled, typesafe_key, openai_key, untriaged, attention, open_items, categories })
}

const ITEM_SELECT: &str = "SELECT i.id,i.message_id,i.kind,i.title,i.due_date,i.due_time,i.end_time,i.location,i.evidence,i.evidence_verified,i.status,i.remind_at,i.reminded_at,i.calendar_event_id,m.subject,m.sender FROM mail_items i JOIN mail_messages m ON m.id=i.message_id";
fn item_row(r: &Row<'_>) -> rusqlite::Result<MailItem> {
    Ok(MailItem {
        id: r.get(0)?,
        message_id: r.get(1)?,
        kind: r.get(2)?,
        title: r.get(3)?,
        due_date: r.get(4)?,
        due_time: r.get(5)?,
        end_time: r.get(6)?,
        location: r.get(7)?,
        evidence: r.get(8)?,
        evidence_verified: r.get(9)?,
        status: r.get(10)?,
        remind_at: r.get(11)?,
        reminded_at: r.get(12)?,
        calendar_event_id: r.get(13)?,
        subject: r.get(14)?,
        sender: r.get(15)?,
    })
}

/// Open items first by due date (undated last); with `message_id`, only that
/// message's items in any status.
pub fn list_items(db: &Database, message_id: Option<i64>, include_closed: bool) -> Result<Vec<MailItem>, String> {
    let conn = db.conn.lock().map_err(err)?;
    let mut stmt = conn.prepare(&format!("{ITEM_SELECT} WHERE (?1 IS NULL OR i.message_id=?1) AND (?2 OR i.status='open') ORDER BY i.status!='open', i.due_date IS NULL, i.due_date, i.due_time IS NULL, i.due_time, i.id LIMIT 500")).map_err(err)?;
    let rows = stmt
        .query_map(params![message_id, include_closed], item_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

pub fn get_item(db: &Database, id: i64) -> Result<MailItem, String> {
    db.conn.lock().map_err(err)?
        .query_row(&format!("{ITEM_SELECT} WHERE i.id=?1"), [id], item_row)
        .map_err(err)
}

/// `remind_at` is a local "YYYY-MM-DDTHH:MM" wall-clock time, or None to clear.
pub fn update_item(db: &Database, id: i64, status: &str, remind_at: Option<&str>) -> Result<MailItem, String> {
    if !["open", "done", "dismissed"].contains(&status) {
        return Err("項目狀態不正確。".into());
    }
    if let Some(at) = remind_at {
        chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M")
            .map_err(|_| "提醒時間格式不正確。".to_string())?;
    }
    required(db.conn.lock().map_err(err)?.execute(
        // Moving the reminder re-arms it; clearing it forgets it was sent.
        "UPDATE mail_items SET status=?2, reminded_at=CASE WHEN remind_at IS ?3 THEN reminded_at ELSE NULL END, remind_at=?3 WHERE id=?1",
        params![id, status, remind_at],
    ).map_err(err)?)?;
    if status != "open" {
        // Finishing the last follow-up takes the message out of "Needs attention".
        db.conn.lock().map_err(err)?.execute(
            "UPDATE mail_triage SET attention=0 WHERE message_id=(SELECT message_id FROM mail_items WHERE id=?1) AND NOT EXISTS(SELECT 1 FROM mail_items o WHERE o.message_id=mail_triage.message_id AND o.status='open')",
            [id],
        ).map_err(err)?;
    }
    get_item(db, id)
}

pub fn set_item_calendar_event(db: &Database, id: i64, event_id: &str) -> Result<(), String> {
    required(db.conn.lock().map_err(err)?
        .execute("UPDATE mail_items SET calendar_event_id=?2 WHERE id=?1", params![id, event_id])
        .map_err(err)?)
}

/// Open items whose reminder time (local wall clock) has arrived and that
/// have not been announced yet. Each item is announced once.
pub fn due_reminders(db: &Database, now_local: &str) -> Result<Vec<MailItem>, String> {
    let conn = db.conn.lock().map_err(err)?;
    let mut stmt = conn.prepare(&format!("{ITEM_SELECT} WHERE i.status='open' AND i.remind_at IS NOT NULL AND i.remind_at<=?1 AND i.reminded_at IS NULL ORDER BY i.remind_at LIMIT 20")).map_err(err)?;
    let rows = stmt
        .query_map([now_local], item_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

pub fn mark_reminded(db: &Database, id: i64) -> Result<(), String> {
    db.conn.lock().map_err(err)?
        .execute("UPDATE mail_items SET reminded_at=datetime('now') WHERE id=?1", [id])
        .map_err(err)?;
    Ok(())
}

/// Which of `provider_ids` this account already has locally.
pub fn known_provider_ids(db: &Database, account_id: &str, provider_ids: &[String]) -> Result<std::collections::HashSet<String>, String> {
    let conn = db.conn.lock().map_err(err)?;
    let mut stmt = conn.prepare("SELECT 1 FROM mail_messages WHERE account_id=?1 AND provider_id=?2").map_err(err)?;
    let mut known = std::collections::HashSet::new();
    for id in provider_ids {
        if stmt.exists(params![account_id, id]).map_err(err)? {
            known.insert(id.clone());
        }
    }
    Ok(known)
}

/// The newest received_at stored for this account (RFC 3339), if any.
pub fn latest_received(db: &Database, account_id: &str) -> Result<Option<String>, String> {
    db.conn.lock().map_err(err)?
        .query_row("SELECT MAX(received_at) FROM mail_messages WHERE account_id=?1 AND received_at LIKE '____-__-__T%'", [account_id], |r| r.get(0))
        .map_err(err)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_booking_and_its_confirmation_yield_one_follow_up() {
        let db = crate::db::test_database();
        upsert_account(&db, "a", "me@example.com", true).unwrap();
        db.conn.lock().unwrap().execute_batch("INSERT INTO mail_messages(id,account_id,provider_id,sender,received_at) VALUES
            (1,'a','p1','Airbnb <express@airbnb.com>','2026-09-20T10:00:00Z'),
            (2,'a','p2','Airbnb <automated@airbnb.com>','2026-09-20T10:05:00Z'),
            (3,'a','p3','Other <x@other.com>','2026-09-20T10:05:00Z');").unwrap();
        let triage = MailTriage { category: "schedule".into(), category_confidence: 0.9, needs_reply: 0.0, action_required: 0.2, has_date: 0.9, importance: 1.6, sent_by_owner: false, attention: true, summary: None, decided_by: "jev".into() };
        let check_in = NewItem { kind: "event".into(), title: "Check in".into(), due_date: Some("2026-10-02".into()), due_time: Some("15:00".into()), end_time: None, location: None, evidence: String::new(), evidence_verified: true };
        save_triage(&db, 1, "Airbnb <express@airbnb.com>", &triage, "jev", None, &[check_in.clone()]).unwrap();
        save_triage(&db, 2, "Airbnb <automated@airbnb.com>", &triage, "jev", None, &[check_in.clone()]).unwrap();
        save_triage(&db, 3, "Other <x@other.com>", &triage, "jev", None, &[check_in]).unwrap();
        assert_eq!(list_items(&db, None, false).unwrap().len(), 2);
        // Re-triaging the first message replaces its own item, not a duplicate.
        assert_eq!(list_items(&db, Some(1), true).unwrap().len(), 1);
        // Two different untimed things due the same day from one domain both stay.
        db.conn.lock().unwrap().execute_batch("INSERT INTO mail_messages(id,account_id,provider_id,sender,received_at) VALUES
            (4,'a','p4','\"a@hkstp.org\" <a@hkstp.org>','2026-09-20T10:00:00Z'),
            (5,'a','p5','Ideation <b@hkstp.org>','2026-09-20T11:00:00Z');").unwrap();
        let session = NewItem { kind: "event".into(), title: "Mentor session".into(), due_date: Some("2026-10-02".into()), due_time: None, end_time: None, location: None, evidence: String::new(), evidence_verified: true };
        let report = NewItem { kind: "deadline".into(), title: "Submit report".into(), ..session.clone() };
        save_triage(&db, 4, "\"a@hkstp.org\" <a@hkstp.org>", &triage, "jev", None, &[session.clone()]).unwrap();
        save_triage(&db, 5, "Ideation <b@hkstp.org>", &triage, "jev", None, &[report, session]).unwrap();
        assert_eq!(list_items(&db, Some(5), true).unwrap().len(), 1, "the repeated session merges, the report stays");
        assert_eq!(sender_domain("\"alice@corp.com\" <alice@corp.com>"), "corp.com");
    }

    #[test]
    fn finishing_the_last_follow_up_clears_needs_attention() {
        let db = crate::db::test_database();
        upsert_account(&db, "a", "me@example.com", true).unwrap();
        db.conn.lock().unwrap().execute("INSERT INTO mail_messages(id,account_id,provider_id,sender,received_at) VALUES (1,'a','p1','x@y.com','2026-09-20T10:00:00Z')", []).unwrap();
        let triage = MailTriage { category: "work".into(), category_confidence: 0.9, needs_reply: 0.9, action_required: 0.9, has_date: 0.0, importance: 2.5, sent_by_owner: false, attention: true, summary: None, decided_by: "jev".into() };
        let task = NewItem { kind: "task".into(), title: "A".into(), due_date: None, due_time: None, end_time: None, location: None, evidence: String::new(), evidence_verified: true };
        save_triage(&db, 1, "x@y.com", &triage, "jev", None, &[task.clone(), NewItem { title: "B".into(), ..task }]).unwrap();
        let items = list_items(&db, Some(1), true).unwrap();
        update_item(&db, items[0].id, "done", None).unwrap();
        assert_eq!(triage_status(&db, None, true, true).unwrap().attention, 1);
        update_item(&db, items[1].id, "dismissed", None).unwrap();
        assert_eq!(triage_status(&db, None, true, true).unwrap().attention, 0);
    }

    use super::*;

    struct FileWorkspace {
        directory: std::path::PathBuf,
    }

    impl FileWorkspace {
        fn new() -> Self {
            let directory =
                std::env::temp_dir().join(format!("jarvis-mail-store-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            Self { directory }
        }

        fn path(&self) -> std::path::PathBuf {
            self.directory.join("workspace.db")
        }

        fn open(&self) -> Database {
            crate::db::test_database_at(&self.path())
        }
    }

    impl Drop for FileWorkspace {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    fn message(provider_id: &str, subject: &str) -> GmailMessage {
        GmailMessage {
            id: provider_id.into(),
            thread_id: Some("thread-one".into()),
            subject: Some(subject.into()),
            sender: Some("Client <client@example.com>".into()),
            snippet: Some("Preview text".into()),
            label_ids: vec!["INBOX".into()],
            is_read: true,
            received_at: Some("2026-09-10T09:30:00Z".into()),
            attachments: Vec::new(),
        }
    }

    fn message_ids(page: &MailPage) -> Vec<i64> {
        page.messages.iter().map(|message| message.id).collect()
    }

    #[test]
    fn file_openai_upgrade_preserves_v22_local_and_bedrock_workspaces() {
        for mode in ["local", "bedrock"] {
            let file = FileWorkspace::new();
            let db = crate::db::test_database_at_version(&file.path(), 22);
            {
                let conn = db.conn.lock().unwrap();
                conn.execute(
                    "UPDATE mail_settings SET processing_mode=?1,local_model='saved-local-model',bedrock_model='saved-bedrock-model' WHERE id=1",
                    [mode],
                ).unwrap();
                conn.execute_batch("INSERT INTO emails(id,gmail_id,sender,received_at,is_archived) VALUES (7,'legacy-inbox','legacy@example.com','2026-08-01',1);
                    INSERT INTO mail_accounts(id,email,connected,last_synced,next_page_token) VALUES ('a','a@example.com',0,'2026-09-10 09:00:00','saved-cursor');
                    INSERT INTO mail_messages(id,account_id,provider_id,subject,body,received_at) VALUES (11,'a','provider-one','Saved subject','Saved body','2026-09-10');
                    INSERT INTO mail_groups(id,name,instructions) VALUES (13,'Saved project','Keep instructions');
                    INSERT INTO mail_group_members(group_id,message_id) VALUES (13,11);
                    INSERT INTO mail_memories(id,group_id,content,kind,source_message_id,status) VALUES (17,13,'Confirmed fact','fact',11,'active');").unwrap();
            }
            drop(db);

            let db = file.open();
            let retained = context(&db, 13).unwrap();
            assert_eq!(retained.settings.processing_mode, mode);
            assert_eq!(retained.settings.local_model, "saved-local-model");
            assert_eq!(retained.settings.bedrock_model, "saved-bedrock-model");
            assert_eq!(
                serde_json::to_value(&retained.settings).unwrap()["openai_model"],
                "gpt-6-luna"
            );
            assert_eq!(retained.group.name, "Saved project");
            assert_eq!(retained.group.instructions, "Keep instructions");
            assert_eq!(retained.group.message_count, 1);
            assert_eq!(retained.messages.len(), 1);
            assert_eq!(retained.messages[0].id, 11);
            assert_eq!(retained.messages[0].subject, "Saved subject");
            assert_eq!(retained.messages[0].body.as_deref(), Some("Saved body"));
            assert_eq!(retained.memories.len(), 1);
            assert_eq!(retained.memories[0].id, 17);
            assert_eq!(retained.memories[0].content, "Confirmed fact");
            assert_eq!(retained.memories[0].source_message_id, Some(11));
            let account = get_account(&db, "a").unwrap();
            assert!(!account.connected);
            assert_eq!(account.last_synced.as_deref(), Some("2026-09-10 09:00:00"));
            assert_eq!(account.next_page_token.as_deref(), Some("saved-cursor"));
            assert_eq!(db.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM emails WHERE id=7 AND gmail_id='legacy-inbox' AND is_archived=1", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
            drop(db);
            let db = file.open();
            assert_eq!(settings(&db).unwrap().processing_mode, mode);
        }
    }

    #[test]
    fn file_openai_settings_are_trimmed_saved_and_reloaded() {
        let file = FileWorkspace::new();
        let db = file.open();
        let input: MailSettings = serde_json::from_value(serde_json::json!({
            "processing_mode": "openai",
            "local_model": " saved-local ",
            "bedrock_model": " saved-bedrock ",
            "openai_model": " gpt-5.6-luna "
        }))
        .unwrap();
        let result = save_settings(&db, &input);
        assert!(
            result.is_ok(),
            "OpenAI settings must be accepted: {result:?}"
        );
        drop(db);
        let db = file.open();
        let saved = serde_json::to_value(settings(&db).unwrap()).unwrap();
        assert_eq!(
            saved,
            serde_json::json!({
                "processing_mode": "openai",
                "local_model": "saved-local",
                "bedrock_model": "saved-bedrock",
                "openai_model": "gpt-5.6-luna"
            })
        );
    }

    #[test]
    fn file_openai_legacy_ipc_defaults_the_model_without_changing_selected_route() {
        let file = FileWorkspace::new();
        let db = file.open();
        let input: MailSettings = serde_json::from_value(serde_json::json!({
            "processing_mode": "local",
            "local_model": "existing-local-model",
            "bedrock_model": "existing-bedrock-model"
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&input).unwrap()["openai_model"],
            "gpt-6-luna"
        );
        save_settings(&db, &input).unwrap();
        drop(db);
        let db = file.open();
        assert_eq!(settings(&db).unwrap().processing_mode, "local");
        assert_eq!(
            serde_json::to_value(settings(&db).unwrap()).unwrap()["openai_model"],
            "gpt-6-luna"
        );
    }

    #[test]
    fn file_openai_model_validation_preserves_saved_settings_on_error() {
        let file = FileWorkspace::new();
        let db = file.open();
        let valid = serde_json::json!({
            "processing_mode": "openai",
            "local_model": "local-model",
            "bedrock_model": "bedrock-model",
            "openai_model": "gpt-5.6-luna"
        });
        save_settings(&db, &serde_json::from_value(valid.clone()).unwrap()).unwrap();
        let mut invalid_model = valid.clone();
        invalid_model["openai_model"] = serde_json::json!("x".repeat(201));
        assert!(save_settings(&db, &serde_json::from_value(invalid_model).unwrap()).is_err());
        let mut invalid_mode = valid.clone();
        invalid_mode["processing_mode"] = serde_json::json!("unknown");
        assert!(save_settings(&db, &serde_json::from_value(invalid_mode).unwrap()).is_err());
        drop(db);
        let db = file.open();
        assert_eq!(serde_json::to_value(settings(&db).unwrap()).unwrap(), valid);
        let mut maximum = valid.clone();
        maximum["openai_model"] = serde_json::json!("x".repeat(200));
        save_settings(&db, &serde_json::from_value(maximum).unwrap()).unwrap();
        let mut empty = valid;
        empty["openai_model"] = serde_json::json!("   ");
        save_settings(&db, &serde_json::from_value(empty).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(settings(&db).unwrap()).unwrap()["openai_model"],
            ""
        );
    }

    // Catches writes that only update process state, lost sync cursors, and UPSERT
    // replacing the local row that group memberships and source notes refer to.
    #[test]
    fn file_workspace_survives_reopening_and_resync_preserves_local_references() {
        let file = FileWorkspace::new();
        let db = file.open();
        upsert_account(&db, "a", "owner@example.com", true).unwrap();
        save_messages(
            &db,
            "a",
            &[(
                message("provider-one", "客戶期限"),
                "Friday deadline".into(),
            )],
        )
        .unwrap();
        let source_id = list_messages(&db, None, None, "", 0, None).unwrap().messages[0].id;
        let group = save_group(&db, None, "客戶工作", "列出已確認嘅期限").unwrap();
        set_group_members(&db, group.id, &[source_id], true).unwrap();
        let memory = save_memory(&db, &note(group.id, Some(source_id))).unwrap();
        finish_sync(&db, "a", Some("page-after-50"), true, true).unwrap();
        finish_sync(&db, "a", Some("recent-page"), false, true).unwrap();
        set_connected(&db, "a", false).unwrap();
        save_settings(
            &db,
            &MailSettings {
                processing_mode: "bedrock".into(),
                local_model: "qwen3:8b".into(),
                bedrock_model: "selected-bedrock-model".into(),
                openai_model: "gpt-5.6-luna".into(),
            },
        )
        .unwrap();
        drop(db);

        let db = file.open();
        let account = get_account(&db, "a").unwrap();
        assert_eq!(account.email, "owner@example.com");
        assert!(!account.connected);
        assert_eq!(account.next_page_token.as_deref(), Some("page-after-50"));
        assert!(account.last_synced.is_some());
        assert_eq!(account.message_count, 1);
        let saved = context(&db, group.id).unwrap();
        assert_eq!(saved.group.name, "客戶工作");
        assert_eq!(saved.group.instructions, "列出已確認嘅期限");
        assert_eq!(saved.group.message_count, 1);
        assert_eq!(saved.memories[0].id, memory.id);
        assert_eq!(saved.memories[0].source_message_id, Some(source_id));
        assert_eq!(saved.settings.processing_mode, "bedrock");
        assert_eq!(saved.settings.local_model, "qwen3:8b");
        assert_eq!(saved.settings.bedrock_model, "selected-bedrock-model");
        let saved_message = &saved.messages[0];
        assert_eq!(saved_message.subject, "客戶期限");
        assert_eq!(saved_message.body.as_deref(), Some("Friday deadline"));
        assert_eq!(saved_message.thread_id.as_deref(), Some("thread-one"));
        assert!(saved_message.is_read && saved_message.is_inbox);

        let mut refreshed = message("provider-one", "Updated deadline");
        refreshed.label_ids.clear();
        refreshed.is_read = false;
        save_messages(&db, "a", &[(refreshed, "Monday deadline".into())]).unwrap();
        finish_sync(&db, "a", None, true, true).unwrap();
        drop(db);

        let db = file.open();
        let saved = context(&db, group.id).unwrap();
        assert_eq!(saved.messages.len(), 1);
        assert_eq!(saved.messages[0].id, source_id);
        assert_eq!(saved.messages[0].subject, "Updated deadline");
        assert_eq!(saved.messages[0].body.as_deref(), Some("Monday deadline"));
        assert!(!saved.messages[0].is_read);
        assert!(!saved.messages[0].is_inbox);
        assert_eq!(saved.memories[0].source_message_id, Some(source_id));
        assert_eq!(get_account(&db, "a").unwrap().next_page_token, None);
    }

    // Catches filters applied after LIMIT, a count query with different filters,
    // duplicate membership joins, and unstable ordering when dates are equal.
    #[test]
    fn file_search_paginates_account_and_group_intersections_without_gaps() {
        let file = FileWorkspace::new();
        let db = file.open();
        upsert_account(&db, "a", "a@example.com", true).unwrap();
        upsert_account(&db, "b", "b@example.com", true).unwrap();
        let project = save_group(&db, None, "Project", "").unwrap();
        let overlap = save_group(&db, None, "Overlap", "").unwrap();
        {
            let mut conn = db.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            for id in 1..=120 {
                tx.execute(
                    "INSERT INTO mail_messages(id,account_id,provider_id,subject,body,received_at) VALUES (?1,?2,?3,'Project needle','Saved body','2026-09-10')",
                    params![id, if id <= 60 { "a" } else { "b" }, format!("provider-{id}")],
                ).unwrap();
            }
            tx.execute_batch("INSERT INTO mail_messages(id,account_id,provider_id,subject,received_at) VALUES (121,'a','unrelated-a','Other work','2026-09-11'),(122,'b','unrelated-b','Other work','2026-09-11');").unwrap();
            tx.commit().unwrap();
        }
        let project_ids: Vec<_> = (1..=55).chain(61..=115).collect();
        set_group_members(&db, project.id, &project_ids, true).unwrap();
        set_group_members(
            &db,
            overlap.id,
            &[1, 2, 3, 4, 5, 116, 117, 118, 119, 120],
            true,
        )
        .unwrap();
        drop(db);
        let db = file.open();

        let cases = [
            (None, None, 120, (1..=120).rev().collect::<Vec<_>>()),
            (Some("a"), None, 60, (1..=60).rev().collect()),
            (Some("b"), None, 60, (61..=120).rev().collect()),
            (
                None,
                Some(project.id),
                110,
                (61..=115).rev().chain((1..=55).rev()).collect(),
            ),
            (Some("a"), Some(project.id), 55, (1..=55).rev().collect()),
            (Some("b"), Some(project.id), 55, (61..=115).rev().collect()),
            (
                None,
                Some(overlap.id),
                10,
                vec![120, 119, 118, 117, 116, 5, 4, 3, 2, 1],
            ),
            (Some("missing"), Some(project.id), 0, vec![]),
        ];
        for (account_id, group_id, total, expected_ids) in cases {
            let mut actual_ids = Vec::new();
            for offset in [0, 50, 100, 150] {
                let page = list_messages(&db, account_id, group_id, "needle", offset, None).unwrap();
                assert_eq!(
                    page.total, total,
                    "account={account_id:?}, group={group_id:?}"
                );
                assert_eq!(
                    page.messages.len(),
                    (total - i64::from(offset)).clamp(0, 50) as usize
                );
                assert!(page.messages.iter().all(|message| message.body.is_none()));
                actual_ids.extend(message_ids(&page));
            }
            assert_eq!(
                actual_ids, expected_ids,
                "account={account_id:?}, group={group_id:?}"
            );
        }
        let empty_tail = list_messages(&db, None, None, "needle", u32::MAX, None).unwrap();
        assert_eq!(empty_tail.total, 120);
        assert!(empty_tail.messages.is_empty());
    }

    // A wildcard escape regression or omission of any searchable field must fail.
    #[test]
    fn file_search_matches_literal_wildcards_backslashes_and_all_text_fields() {
        let file = FileWorkspace::new();
        let db = file.open();
        upsert_account(&db, "a", "a@example.com", true).unwrap();
        db.conn.lock().unwrap().execute_batch(
            r#"INSERT INTO mail_messages(id,account_id,provider_id,subject,sender,snippet,body,received_at) VALUES
            (1,'a','subject','100%_paid\invoice','','','','2026-09-10'),
            (2,'a','sender','','100%_paid\invoice@example.com','','','2026-09-10'),
            (3,'a','snippet','','','100%_paid\invoice','','2026-09-10'),
            (4,'a','body','','','','100%_paid\invoice','2026-09-10'),
            (5,'a','near-match','100Xpaidinvoice','','','','2026-09-10');"#,
        ).unwrap();
        let page = list_messages(&db, Some("a"), None, r"  100%_paid\invoice  ", 0, None).unwrap();
        assert_eq!(page.total, 4);
        assert_eq!(message_ids(&page), vec![4, 3, 2, 1]);
        assert_eq!(
            list_messages(&db, None, None, "' OR 1=1 --", 0, None)
                .unwrap()
                .total,
            0
        );
        assert_eq!(list_messages(&db, None, None, "   ", 0, None).unwrap().total, 5);
        assert!(list_messages(&db, None, None, &"字".repeat(500), 0, None).is_ok());
        assert!(list_messages(&db, None, None, &"字".repeat(501), 0, None).is_err());
    }

    // Catches partial bulk writes in both directions. The trigger represents a
    // real SQLite write error on the second removal, after the first succeeded.
    #[test]
    fn file_group_membership_rolls_back_failed_additions_and_removals() {
        let file = FileWorkspace::new();
        let db = file.open();
        upsert_account(&db, "a", "a@example.com", true).unwrap();
        for id in ["one", "two", "three"] {
            save_messages(&db, "a", &[(message(id, id), "body".into())]).unwrap();
        }
        let group = save_group(&db, None, "Project", "").unwrap();
        set_group_members(&db, group.id, &[1], true).unwrap();
        assert!(set_group_members(&db, group.id, &[2, 999, 3], true).is_err());
        drop(db);
        let db = file.open();
        assert_eq!(
            message_ids(&list_messages(&db, None, Some(group.id), "", 0, None).unwrap()),
            vec![1]
        );
        set_group_members(&db, group.id, &[2, 2, 3], true).unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_second_removal BEFORE DELETE ON mail_group_members WHEN OLD.message_id=2 BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END;").unwrap();
        assert!(set_group_members(&db, group.id, &[1, 2], false).is_err());
        drop(db);
        let db = file.open();
        assert_eq!(
            message_ids(&list_messages(&db, None, Some(group.id), "", 0, None).unwrap()),
            vec![3, 2, 1]
        );
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_second_removal;")
            .unwrap();
        set_group_members(&db, group.id, &[1, 2], false).unwrap();
        assert_eq!(
            message_ids(&list_messages(&db, None, Some(group.id), "", 0, None).unwrap()),
            vec![3]
        );
        assert_eq!(list_messages(&db, None, None, "", 0, None).unwrap().total, 3);
    }

    // Catches notes lost during editing, the expiry date being treated as already
    // expired today, and removed sources continuing to influence analysis.
    #[test]
    fn file_memory_edits_expiry_and_source_membership_survive_reopening() {
        let file = FileWorkspace::new();
        let db = file.open();
        upsert_account(&db, "a", "a@example.com", true).unwrap();
        save_messages(&db, "a", &[(message("one", "Project"), "body".into())]).unwrap();
        let group = save_group(&db, None, "Project", "").unwrap();
        set_group_members(&db, group.id, &[1], true).unwrap();
        let today = chrono::Local::now().date_naive();
        let mut input = note(group.id, Some(1));
        input.expires_at = Some(today.to_string());
        let saved = save_memory(&db, &input).unwrap();
        input.id = Some(saved.id);
        input.content = "Updated confirmed deadline".into();
        input.kind = "fact".into();
        save_memory(&db, &input).unwrap();
        drop(db);
        let db = file.open();
        let active = context(&db, group.id).unwrap().memories;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, saved.id);
        assert_eq!(active[0].content, "Updated confirmed deadline");
        assert_eq!(active[0].kind, "fact");
        assert_eq!(
            active[0].expires_at.as_deref(),
            Some(today.to_string().as_str())
        );

        input.expires_at = Some(today.pred_opt().unwrap().to_string());
        save_memory(&db, &input).unwrap();
        assert!(context(&db, group.id).unwrap().memories.is_empty());
        assert_eq!(list_memories(&db, group.id).unwrap().len(), 1);
        input.expires_at = Some(today.succ_opt().unwrap().to_string());
        input.status = "done".into();
        save_memory(&db, &input).unwrap();
        assert!(context(&db, group.id).unwrap().memories.is_empty());
        input.status = "active".into();
        save_memory(&db, &input).unwrap();
        set_group_members(&db, group.id, &[1], false).unwrap();
        drop(db);
        let db = file.open();
        assert!(context(&db, group.id).unwrap().memories.is_empty());
        let retained = list_memories(&db, group.id).unwrap();
        assert_eq!(retained[0].source_message_id, Some(1));
        assert_eq!(retained[0].content, "Updated confirmed deadline");
        set_group_members(&db, group.id, &[1], true).unwrap();
        assert_eq!(context(&db, group.id).unwrap().memories[0].id, saved.id);

        for bad_date in ["2026-02-29", "2026-9-10", "2026-09-10T00:00:00Z"] {
            input.expires_at = Some(bad_date.into());
            assert!(save_memory(&db, &input).is_err());
            assert_eq!(
                list_memories(&db, group.id).unwrap()[0].expires_at,
                Some(today.succ_opt().unwrap().to_string())
            );
        }
        input.source_message_id = None;
        input.expires_at = Some(String::new());
        save_memory(&db, &input).unwrap();
        set_group_members(&db, group.id, &[1], false).unwrap();
        drop(db);
        let db = file.open();
        let active = context(&db, group.id).unwrap().memories;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].source_message_id, None);
        assert_eq!(active[0].expires_at, None);
        delete_memory(&db, saved.id).unwrap();
        drop(db);
        let db = file.open();
        assert!(list_memories(&db, group.id).unwrap().is_empty());
    }

    // Catches a shared provider-id key, overbroad account deletion, lost manual
    // notes, or deleting notes before a failed account deletion can roll back.
    #[test]
    fn file_account_deletion_is_atomic_and_preserves_other_account_data() {
        let file = FileWorkspace::new();
        let db = file.open();
        upsert_account(&db, "a", "a@example.com", true).unwrap();
        upsert_account(&db, "b", "b@example.com", true).unwrap();
        save_messages(
            &db,
            "a",
            &[(message("same-id", "Account A"), "Body A".into())],
        )
        .unwrap();
        save_messages(
            &db,
            "b",
            &[(message("same-id", "Account B"), "Body B".into())],
        )
        .unwrap();
        save_messages(
            &db,
            "a",
            &[(
                message("same-id", "Updated account A"),
                "Updated body A".into(),
            )],
        )
        .unwrap();
        let group = save_group(&db, None, "Shared project", "Retain instructions").unwrap();
        let other = save_group(&db, None, "Other project", "").unwrap();
        set_group_members(&db, group.id, &[1, 2], true).unwrap();
        set_group_members(&db, other.id, &[2], true).unwrap();
        let from_a = save_memory(&db, &note(group.id, Some(1))).unwrap();
        let from_b = save_memory(&db, &note(group.id, Some(2))).unwrap();
        let manual = save_memory(&db, &note(group.id, None)).unwrap();
        let other_note = save_memory(&db, &note(other.id, Some(2))).unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_account_deletion BEFORE DELETE ON mail_accounts WHEN OLD.id='a' BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END;").unwrap();
        assert!(delete_account(&db, "a").is_err());
        drop(db);

        let db = file.open();
        assert_eq!(
            get_message(&db, 1).unwrap().body.as_deref(),
            Some("Updated body A")
        );
        assert_eq!(get_message(&db, 2).unwrap().body.as_deref(), Some("Body B"));
        assert_eq!(
            list_memories(&db, group.id)
                .unwrap()
                .iter()
                .map(|memory| memory.id)
                .collect::<Vec<_>>(),
            vec![manual.id, from_b.id, from_a.id]
        );
        assert_eq!(context(&db, group.id).unwrap().group.message_count, 2);
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_account_deletion;")
            .unwrap();
        delete_account(&db, "a").unwrap();
        drop(db);

        let db = file.open();
        assert!(get_account(&db, "a").is_err());
        assert!(get_message(&db, 1).is_err());
        assert_eq!(list_accounts(&db).unwrap().len(), 1);
        assert_eq!(get_account(&db, "b").unwrap().message_count, 1);
        assert_eq!(get_message(&db, 2).unwrap().subject, "Account B");
        assert_eq!(get_message(&db, 2).unwrap().body.as_deref(), Some("Body B"));
        let retained = context(&db, group.id).unwrap();
        assert_eq!(retained.group.message_count, 1);
        assert_eq!(retained.group.instructions, "Retain instructions");
        assert_eq!(
            retained
                .memories
                .iter()
                .map(|memory| memory.id)
                .collect::<Vec<_>>(),
            vec![manual.id, from_b.id]
        );
        assert_eq!(
            context(&db, other.id).unwrap().memories[0].id,
            other_note.id
        );
        delete_group(&db, group.id).unwrap();
        drop(db);
        let db = file.open();
        assert_eq!(list_groups(&db).unwrap().len(), 1);
        assert_eq!(context(&db, other.id).unwrap().messages[0].id, 2);
        assert!(list_memories(&db, group.id).is_err());
        assert_eq!(list_messages(&db, None, None, "", 0, None).unwrap().total, 1);
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM mail_memories WHERE group_id=?1",
                [group.id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM mail_group_members WHERE group_id=?1",
                [group.id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    // Catches applying the creation cap to edits, or validating by UTF-8 bytes
    // instead of characters, which rejects valid Cantonese notes prematurely.
    #[test]
    fn file_memory_limit_allows_editing_and_freeing_capacity_at_the_boundary() {
        let file = FileWorkspace::new();
        let db = file.open();
        let group = save_group(&db, None, &"組".repeat(100), "").unwrap();
        assert!(save_group(&db, None, &"組".repeat(101), "").is_err());
        let mut input = note(group.id, None);
        input.content = "字".repeat(2000);
        let first = save_memory(&db, &input).unwrap();
        {
            let mut conn = db.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            for _ in 1..200 {
                tx.execute("INSERT INTO mail_memories(group_id,content,kind,status) VALUES (?1,'Existing note','fact','done')", [group.id]).unwrap();
            }
            tx.commit().unwrap();
        }
        assert!(save_memory(&db, &note(group.id, None)).is_err());
        input.id = Some(first.id);
        input.content = "Edited at full capacity".into();
        save_memory(&db, &input).unwrap();
        input.content = "字".repeat(2001);
        assert!(save_memory(&db, &input).is_err());
        drop(db);
        let db = file.open();
        let memories = list_memories(&db, group.id).unwrap();
        assert_eq!(memories.len(), 200);
        assert_eq!(
            memories
                .iter()
                .find(|memory| memory.id == first.id)
                .unwrap()
                .content,
            "Edited at full capacity"
        );
        delete_memory(&db, first.id).unwrap();
        save_memory(&db, &note(group.id, None)).unwrap();
        assert_eq!(list_memories(&db, group.id).unwrap().len(), 200);
    }

    #[test]
    fn expired_notes_remain_editable_but_removed_sources_leave_analysis() {
        let db = seeded();
        let group = save_group(&db, None, "Source", "").unwrap();
        set_group_members(&db, group.id, &[1, 2], true).unwrap();
        let saved = save_memory(&db, &note(group.id, Some(1))).unwrap();
        set_group_members(&db, group.id, &[1], false).unwrap();
        assert_eq!(list_memories(&db, group.id).unwrap().len(), 1);
        assert!(context(&db, group.id).unwrap().memories.is_empty());
        let mut input = note(group.id, Some(2));
        input.id = Some(saved.id);
        input.expires_at = Some("2000-01-01".into());
        save_memory(&db, &input).unwrap();
        assert!(context(&db, group.id).unwrap().memories.is_empty());
        input.expires_at = None;
        save_memory(&db, &input).unwrap();
        assert_eq!(context(&db, group.id).unwrap().memories.len(), 1);
    }
    #[test]
    fn settings_reject_unknown_routes_without_overwriting_local_preference() {
        let db = seeded();
        assert!(save_settings(
            &db,
            &MailSettings {
                processing_mode: "gemini".into(),
                local_model: "local".into(),
                bedrock_model: "cloud".into(),
                openai_model: "gpt-5.6-luna".into(),
            }
        )
        .is_err());
        assert_eq!(settings(&db).unwrap().processing_mode, "local");
        save_settings(
            &db,
            &MailSettings {
                processing_mode: "bedrock".into(),
                local_model: "qwen3:8b".into(),
                bedrock_model: "model".into(),
                openai_model: "gpt-5.6-luna".into(),
            },
        )
        .unwrap();
        assert_eq!(settings(&db).unwrap().local_model, "qwen3:8b");
        assert_eq!(settings(&db).unwrap().processing_mode, "bedrock");
    }
    fn seeded() -> Database {
        let db = crate::db::test_database();
        db.conn.lock().unwrap().execute_batch("INSERT INTO mail_accounts(id,email) VALUES ('a','a@example.com'),('b','b@example.com');
          INSERT INTO mail_messages(id,account_id,provider_id,subject,sender,body,received_at) VALUES
          (1,'a','same','100% paid','one','Budget 100% approved','2026-09-10'),
          (2,'b','same','Second project','two','Private second project','2026-09-09');").unwrap();
        db
    }
    #[test]
    fn deleting_a_group_keeps_mail_and_other_groups() {
        let db = seeded();
        let first = save_group(&db, None, "工作", "Deadline first").unwrap();
        let second = save_group(&db, None, "另一組", "").unwrap();
        set_group_members(&db, first.id, &[1, 2], true).unwrap();
        set_group_members(&db, second.id, &[1], true).unwrap();
        delete_group(&db, first.id).unwrap();
        assert_eq!(list_messages(&db, None, None, "", 0, None).unwrap().total, 2);
        assert_eq!(
            list_messages(&db, None, Some(second.id), "", 0, None)
                .unwrap()
                .messages[0]
                .id,
            1
        );
    }
    #[test]
    fn invalid_bulk_selection_rolls_back_and_repeated_add_is_idempotent() {
        let db = seeded();
        let group = save_group(&db, None, "Project", "").unwrap();
        assert!(set_group_members(&db, group.id, &[1, 999], true).is_err());
        assert_eq!(
            list_messages(&db, None, Some(group.id), "", 0, None)
                .unwrap()
                .total,
            0
        );
        set_group_members(&db, group.id, &[1, 1], true).unwrap();
        set_group_members(&db, group.id, &[1], true).unwrap();
        assert_eq!(
            list_messages(&db, None, Some(group.id), "", 0, None)
                .unwrap()
                .total,
            1
        );
    }
    #[test]
    fn search_treats_wildcards_literally_and_scopes_account_and_group() {
        let db = seeded();
        assert_eq!(list_messages(&db, None, None, "%", 0, None).unwrap().total, 1);
        assert_eq!(
            list_messages(&db, Some("b"), None, "100%", 0, None)
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            list_messages(&db, None, None, "Private", 0, None)
                .unwrap()
                .messages[0]
                .id,
            2
        );
        assert!(list_messages(&db, None, None, "", 0, None).unwrap().messages[0]
            .body
            .is_none());
    }
    fn note(group_id: i64, source: Option<i64>) -> MailMemoryInput {
        MailMemoryInput {
            id: None,
            group_id,
            content: "Client deadline".into(),
            kind: "commitment".into(),
            source_message_id: source,
            expires_at: None,
            status: "active".into(),
        }
    }
    #[test]
    fn memory_sources_must_belong_to_the_group_and_edits_cannot_move_between_groups() {
        let db = seeded();
        let a = save_group(&db, None, "A", "").unwrap();
        let b = save_group(&db, None, "B", "").unwrap();
        set_group_members(&db, a.id, &[1], true).unwrap();
        assert!(save_memory(&db, &note(a.id, Some(2))).is_err());
        let saved = save_memory(&db, &note(a.id, Some(1))).unwrap();
        let mut edit = note(b.id, None);
        edit.id = Some(saved.id);
        assert!(save_memory(&db, &edit).is_err());
    }
    #[test]
    fn analysis_context_excludes_other_group_done_and_expired_memories() {
        let db = seeded();
        let a = save_group(&db, None, "A", "User instruction").unwrap();
        let b = save_group(&db, None, "B", "").unwrap();
        set_group_members(&db, a.id, &[1], true).unwrap();
        set_group_members(&db, b.id, &[2], true).unwrap();
        save_memory(&db, &note(a.id, Some(1))).unwrap();
        let mut expired = note(a.id, None);
        expired.expires_at = Some("2000-01-01".into());
        save_memory(&db, &expired).unwrap();
        let mut done = note(a.id, None);
        done.status = "done".into();
        save_memory(&db, &done).unwrap();
        save_memory(&db, &note(b.id, Some(2))).unwrap();
        let context = context(&db, a.id).unwrap();
        assert_eq!(
            context.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(context.memories.len(), 1);
        assert_eq!(context.settings.processing_mode, "local");
        assert_eq!(context.group.instructions, "User instruction");
    }
    /// Messages saved before attachments were recorded only gain them when a
    /// rescan saves them again, so the upsert has to replace the list.
    #[test]
    fn attachments_are_saved_read_back_and_replaced_by_a_rescan() {
        let db = seeded();
        let mut msg = message("with-file", "Invoice");
        msg.attachments = vec![MailAttachment { filename: "Invoice 0925.pdf".into(), mime_type: "application/pdf".into(), size: 184320 }];
        save_messages(&db, "a", &[(msg.clone(), "body".into())]).unwrap();

        let listed = list_messages(&db, Some("a"), None, "Invoice", 0, None).unwrap();
        assert_eq!(listed.messages[0].attachments.as_ref(), Some(&msg.attachments), "the inbox list needs the paperclip without opening the message");
        let opened = get_message(&db, listed.messages[0].id).unwrap();
        assert_eq!(opened.attachments.as_ref(), Some(&msg.attachments));

        msg.attachments.clear();
        save_messages(&db, "a", &[(msg, "body".into())]).unwrap();
        assert_eq!(get_message(&db, opened.id).unwrap().attachments, Some(Vec::new()), "a rescan that finds none records none");

        // Rows written before V26 are unknown, not empty.
        assert_eq!(get_message(&db, 1).unwrap().attachments, None);
    }

    #[test]
    fn sync_updates_body_without_replacing_local_message_identity_or_membership() {
        let db = seeded();
        let group = save_group(&db, None, "Project", "").unwrap();
        set_group_members(&db, group.id, &[1], true).unwrap();
        let msg = GmailMessage {
            id: "same".into(),
            thread_id: None,
            subject: Some("Updated".into()),
            sender: Some("one".into()),
            snippet: None,
            label_ids: vec!["INBOX".into()],
            is_read: true,
            received_at: Some("2026-09-10".into()),
            attachments: Vec::new(),
        };
        save_messages(&db, "a", &[(msg.clone(), "new body".into())]).unwrap();
        save_messages(&db, "a", &[(msg, "new body".into())]).unwrap();
        let context = context(&db, group.id).unwrap();
        assert_eq!(context.messages[0].id, 1);
        assert_eq!(context.messages[0].body.as_deref(), Some("new body"));
        assert_eq!(list_messages(&db, None, None, "", 0, None).unwrap().total, 2);
    }
    // Unlike inserting an old-format row after all migrations, this catches an
    // upgrade that drops or rewrites a pre-existing inbox or user preference.
    #[test]
    fn file_upgrade_from_v21_preserves_existing_inbox_and_preferences() {
        let file = FileWorkspace::new();
        let db = crate::db::test_database_at_version(&file.path(), 21);
        {
            let conn = db.conn.lock().unwrap();
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name='mail_accounts'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            conn.execute_batch("INSERT INTO emails(id,gmail_id,thread_id,subject,sender,snippet,is_read,is_archived,received_at) VALUES (7,'legacy','old-thread','舊信件','old@example.com','Keep this preview',1,1,'2026-08-01');
                UPDATE user_preferences SET value='gemini_primary' WHERE key='ai_provider';
                INSERT INTO user_preferences(key,value) VALUES ('mail-upgrade-fixture','User preference');").unwrap();
        }
        drop(db);

        let db = file.open();
        {
            let conn = db.conn.lock().unwrap();
            let old = conn.query_row("SELECT id,gmail_id,thread_id,subject,sender,snippet,is_read,is_archived,received_at FROM emails", [], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, bool>(6)?, row.get::<_, bool>(7)?, row.get::<_, String>(8)?))
            }).unwrap();
            assert_eq!(
                old,
                (
                    7,
                    "legacy".into(),
                    "old-thread".into(),
                    "舊信件".into(),
                    "old@example.com".into(),
                    "Keep this preview".into(),
                    true,
                    true,
                    "2026-08-01".into()
                )
            );
            assert_eq!(
                conn.query_row(
                    "SELECT value FROM user_preferences WHERE key='ai_provider'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
                "gemini_primary"
            );
            assert_eq!(
                conn.query_row(
                    "SELECT value FROM user_preferences WHERE key='mail-upgrade-fixture'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
                "User preference"
            );
        }
        assert!(list_accounts(&db).unwrap().is_empty());
        assert_eq!(list_messages(&db, None, None, "", 0, None).unwrap().total, 0);
        assert_eq!(settings(&db).unwrap().processing_mode, "local");
        let group = save_group(&db, None, "Created after upgrade", "").unwrap();
        drop(db);
        let db = file.open();
        assert_eq!(list_groups(&db).unwrap()[0].id, group.id);
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM emails WHERE gmail_id='legacy' AND is_archived=1",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }

    // Catches source validation accidentally being limited to the fetched body
    // window instead of all current members of this group.
    #[test]
    fn file_context_retains_active_memory_from_a_source_beyond_the_message_window() {
        let file = FileWorkspace::new();
        let db = file.open();
        upsert_account(&db, "a", "a@example.com", true).unwrap();
        let group = save_group(&db, None, "Large project", "").unwrap();
        {
            let mut conn = db.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            for id in 1..=101 {
                tx.execute("INSERT INTO mail_messages(id,account_id,provider_id,subject,body,received_at) VALUES (?1,'a',?2,'Project mail','Saved body','2026-09-10')", params![id, format!("provider-{id}")]).unwrap();
            }
            tx.commit().unwrap();
        }
        set_group_members(&db, group.id, &(1..=101).collect::<Vec<_>>(), true).unwrap();
        let memory = save_memory(&db, &note(group.id, Some(1))).unwrap();
        drop(db);
        let db = file.open();
        let loaded = context(&db, group.id).unwrap();
        assert_eq!(loaded.group.message_count, 101);
        assert_eq!(loaded.messages.len(), 100);
        assert_eq!(loaded.messages.first().unwrap().id, 101);
        assert_eq!(loaded.messages.last().unwrap().id, 2);
        assert_eq!(loaded.memories.len(), 1);
        assert_eq!(loaded.memories[0].id, memory.id);
        assert_eq!(loaded.memories[0].source_message_id, Some(1));
    }
}
