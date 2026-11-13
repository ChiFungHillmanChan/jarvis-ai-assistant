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
        ("onboarding_complete", if std::env::var("JARVIS_NATIVE_QA_FIRST_RUN").as_deref() == Ok("1") { "false" } else { "true" }),
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
    // Attachment and sender rendering: qa-001 has two files and a stored header
    // block, qa-002 was scanned and has none, every other row predates V26
    // (attachments NULL) and keeps the generic Gmail note.
    tx.execute("UPDATE mail_messages SET sender=?1, attachments=?2, body=?3 WHERE provider_id='qa-001'", params![
        "陳大文 <tai.man@example.test>",
        r#"[{"filename":"報價單 2026-09.pdf","mime_type":"application/pdf","size":184320},{"filename":"site-photo.jpg","mime_type":"image/jpeg","size":3250000}]"#,
        "From: 陳大文 <tai.man@example.test>\nTo: personal@example.test\nDate: Thu, 24 Sep 2026 10:00:00 +0800\nSubject: 桌面測試郵件 001\n\n請於星期五前確認附件中的報價單。",
    ])?;
    tx.execute("UPDATE mail_messages SET attachments='[]' WHERE provider_id='qa-002'", [])?;
    // Follow-ups on Home: overdue, due today, in two days, and one outside the
    // three-day window that must not appear.
    for (title, offset) in [("合成：回覆報價", "-1 day"), ("合成：提交試用紀錄", "+0 days"), ("合成：跟進試用客戶", "+2 days"), ("合成：季度檢討", "+10 days")] {
        tx.execute("INSERT INTO tasks(title,deadline,priority,status) VALUES (?1,date('now','localtime',?2),1,'pending')", params![title, offset])?;
    }
    if std::env::var("JARVIS_NATIVE_QA_PRESENTATION").as_deref() == Ok("1") {
        // This stored fixture checks rendering and source navigation only. It
        // is explicitly not evidence of a live AI analysis or attachment read.
        tx.execute("INSERT INTO mail_groups(id,name,instructions) VALUES (1,?1,?2)",
            params!["排版測試（合成資料）", "只供原生排版驗收；此結果並非模型生成。"])?;
        tx.execute("INSERT INTO mail_group_members(group_id,message_id) VALUES (1,1),(1,2)", [])?;
        let layout = serde_json::json!({"tabs":[
            {"id":"automation","view":"home","title":"Automation","pinned":true},
            {"id":"tool:email","view":"email","title":"Mail"}
        ],"activeId":"tool:email","history":[]});
        let pages = serde_json::json!({"tool:email":{"mailGroupId":1,"mailAnalysis":{"groupId":1,"result":{
            "text":"# 排版驗收示範\n\n這是**合成結果**，只檢查排版。\n\n| 文件 | 截止日期 | 來源 |\n| --- | --- | --- |\n| 試用報告 | 2026-09-30 | [郵件 #1] |\n| 費用資料 | 待確認 | [郵件 #2] |\n\n- 核對原信\n- 取得原始文件\n\n> 此結果不是模型驗收證據。\n\n```text\n原始程式碼符號 **保留**\n```",
            "mode":"local","model":"synthetic-rendering-fixture","source_ids":[1,2],"omitted_messages":0
        }}}});
        for (key, value) in [("workspace_layout_v1",layout.to_string()),("workspace_pages_v1",pages.to_string())] {
            tx.execute("INSERT OR REPLACE INTO user_preferences(key,value) VALUES (?1,?2)", params![key,value])?;
        }
    }
    // AI usage of several ages and kinds (ai/usage.rs), then the roll-up the
    // launch already ran before this seed, so the first QA launch shows both
    // the requests and the daily totals.
    crate::ai::usage::seed_for_qa(&tx, chrono::Utc::now().timestamp())?;
    tx.commit()?;
    crate::ai::usage::roll_up_on_open(conn);
    Ok(())
}
