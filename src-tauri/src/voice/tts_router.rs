use crate::db::Database;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TtsMode { Ack, Content }

#[derive(Clone, Debug, PartialEq)]
pub struct TtsRequest {
    pub text: String,
    pub mode: TtsMode,
    pub language: String,  // "en" | "yue"
}

#[derive(Clone, Debug, PartialEq)]
pub enum TtsProvider {
    MacSay { voice: String },
    Minimax,
    CosyVoice,
}

pub struct TtsRouter {
    db: Arc<Database>,
}

impl TtsRouter {
    pub fn new(db: Arc<Database>) -> Self { Self { db } }

    pub fn select_provider(&self, req: &TtsRequest) -> Result<TtsProvider, String> {
        let conn = self.db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
        let read = |k: &str| -> String {
            conn.query_row(
                "SELECT value FROM user_preferences WHERE key = ?1",
                rusqlite::params![k], |r| r.get::<_, String>(0),
            ).unwrap_or_default()
        };
        // Acks always use macOS say (free, instant)
        if req.mode == TtsMode::Ack {
            let voice = match req.language.as_str() {
                "yue" => read("tts_voice_yue"),
                _     => read("tts_voice_en"),
            };
            let voice = if voice.is_empty() { "Samantha".into() } else { voice };
            return Ok(TtsProvider::MacSay { voice });
        }
        // Content respects offline mode + provider pref
        if read("tts_offline_mode") == "1" {
            return Ok(TtsProvider::CosyVoice);
        }
        match read("tts_content_provider").as_str() {
            "minimax"   => Ok(TtsProvider::Minimax),
            "cosyvoice" => Ok(TtsProvider::CosyVoice),
            _ => {
                let voice = match req.language.as_str() {
                    "yue" => read("tts_voice_yue"),
                    _     => read("tts_voice_en"),
                };
                Ok(TtsProvider::MacSay { voice: if voice.is_empty() { "Samantha".into() } else { voice } })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn fresh_db_with_prefs() -> Arc<Database> {
        // In memory, not $TMPDIR: nothing here exercises file-backed behaviour,
        // and the on-disk fixture left an orphan .db (plus -wal/-shm) behind on
        // every `cargo test` run because nothing ever removed them.
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS user_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            INSERT OR IGNORE INTO user_preferences (key, value) VALUES
                ('language',             'en'),
                ('stt_language',         'auto'),
                ('tts_voice_en',         'Samantha'),
                ('tts_voice_yue',        'Sin-ji'),
                ('tts_content_provider', 'sin-ji'),
                ('tts_offline_mode',     '0'),
                ('minimax_api_key',      ''),
                ('cosyvoice_model_path', '');"
        ).expect("seed prefs");
        Arc::new(Database { conn: Mutex::new(conn) })
    }

    #[test]
    fn ack_yue_uses_sin_ji() {
        let db = fresh_db_with_prefs();
        let r = TtsRouter::new(db);
        let p = r.select_provider(&TtsRequest {
            text: "已完成".into(), mode: TtsMode::Ack, language: "yue".into(),
        }).unwrap();
        assert_eq!(p, TtsProvider::MacSay { voice: "Sin-ji".into() });
    }

    #[test]
    fn ack_en_uses_samantha() {
        let db = fresh_db_with_prefs();
        let r = TtsRouter::new(db);
        let p = r.select_provider(&TtsRequest {
            text: "Done".into(), mode: TtsMode::Ack, language: "en".into(),
        }).unwrap();
        assert_eq!(p, TtsProvider::MacSay { voice: "Samantha".into() });
    }

    #[test]
    fn content_offline_uses_cosyvoice() {
        let db = fresh_db_with_prefs();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "UPDATE user_preferences SET value='1' WHERE key='tts_offline_mode'",
                [],
            ).unwrap();
        }
        let r = TtsRouter::new(db);
        let p = r.select_provider(&TtsRequest {
            text: "long content".into(), mode: TtsMode::Content, language: "yue".into(),
        }).unwrap();
        assert_eq!(p, TtsProvider::CosyVoice);
    }
}
