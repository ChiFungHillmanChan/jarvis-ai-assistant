//! Explicit native QA build. IPC and storage are the production implementation.
//! Only startup fixtures, database location and credential storage differ.

#[cfg(not(debug_assertions))]
compile_error!("native-qa is for debug builds only; never package it for release");

use rusqlite::{params, Connection};
use std::path::PathBuf;

pub fn directory() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::var("JARVIS_NATIVE_QA_DIR")?);
    if !path.is_absolute() {
        return Err("native QA requires an absolute temporary directory".into());
    }
    let path = path.canonicalize()?;
    let temporary = std::env::temp_dir().canonicalize()?;
    if !path.starts_with(&temporary)
        || path == temporary
        || !path.join(".jarvis-native-qa").is_file()
    {
        return Err(
            "native QA requires a marked subdirectory of the system temporary directory".into(),
        );
    }
    Ok(path)
}

pub fn seed_once(conn: &mut Connection) -> Result<(), rusqlite::Error> {
    let seeded: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM user_preferences WHERE key='native_qa_seeded')",
        [],
        |r| r.get(0),
    )?;
    if seeded {
        return Ok(());
    }
    let tx = conn.transaction()?;
    for (key, value) in [
        ("onboarding_complete", "true"),
        ("auto_briefing", "false"),
        ("wake_word_enabled", "false"),
        ("ai_provider", "local"),
        ("native_qa_seeded", "true"),
    ] {
        tx.execute(
            "INSERT OR REPLACE INTO user_preferences(key,value) VALUES (?1,?2)",
            params![key, value],
        )?;
    }
    tx.execute("UPDATE cron_jobs SET status='paused'", [])?;
    tx.execute("DELETE FROM provider_chain", [])?;
    for (id, email) in [
        ("qa-personal", "personal@example.test"),
        ("qa-work", "work@example.test"),
    ] {
        tx.execute(
            "INSERT INTO mail_accounts(id,email,connected) VALUES (?1,?2,0)",
            params![id, email],
        )?;
    }
    for index in 1..=120 {
        let account = if index % 2 == 0 {
            "qa-work"
        } else {
            "qa-personal"
        };
        let body = format!("呢封係人工建立嘅桌面測試郵件 {index:03}。\n項目：香港客戶試用\n到期：2026-09-30\n金額：HK$148\n獨特搜尋字：native-body-{index:03}\n<script>alert('untrusted-email')</script>");
        tx.execute("INSERT INTO mail_messages(account_id,provider_id,subject,sender,snippet,body,is_read,is_inbox,received_at) VALUES (?1,?2,?3,?4,?5,?5,?6,1,?7)",
            params![account,format!("qa-{index:03}"),format!("桌面測試郵件 {index:03}"),"客戶 <customer@example.test>",body,index%3==0,
                format!("2026-09-10T{:02}:{:02}:00Z",index/60,index%60)])?;
    }
    tx.commit()
}
