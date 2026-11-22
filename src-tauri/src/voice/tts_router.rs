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
                // The old Settings page saved the English voice as `tts_voice` only.
                _     => Some(read("tts_voice_en")).filter(|v| !v.is_empty()).unwrap_or_else(|| read("tts_voice")),
            };
            let voice = if voice.is_empty() { "Samantha".into() } else { voice };
            return Ok(TtsProvider::MacSay { voice });
        }
        // Content respects offline mode + provider pref
        if read("tts_offline_mode") == "1" {
            return Ok(TtsProvider::CosyVoice);
        }
        match read("tts_content_provider").as_str() {
            // Content can be mail and calendar text (the briefing): a cloud
            // voice gets it only when the registry marks its terms safe,
            // otherwise the system voice reads it (P06; cloud to local only,
            // as for speech to text).
            "minimax" if terms_are_safe("minimax") => Ok(TtsProvider::Minimax),
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

/// Whether the registry marks `provider`'s data terms safe now. A provider
/// it does not list is not.
fn terms_are_safe(provider: &str) -> bool {
    crate::ai::registry::current().provider(provider).is_some_and(|p| p.data_terms == crate::ai::registry::DataTerms::Safe)
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

    /// The old Settings page saved the English voice as `tts_voice` only, so
    /// it is the fallback when `tts_voice_en` is empty.
    #[test]
    fn ack_voice_falls_back_to_tts_voice() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE user_preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO user_preferences (key, value) VALUES ('tts_voice', 'Daniel');"
        ).expect("seed prefs");
        let db = Arc::new(Database { conn: Mutex::new(conn) });
        let ack = TtsRequest { text: "Done".into(), mode: TtsMode::Ack, language: "en".into() };
        let r = TtsRouter::new(db.clone());
        assert_eq!(r.select_provider(&ack).unwrap(), TtsProvider::MacSay { voice: "Daniel".into() });
        db.conn.lock().unwrap().execute("INSERT INTO user_preferences (key, value) VALUES ('tts_voice_en', 'Karen')", []).unwrap();
        assert_eq!(r.select_provider(&ack).unwrap(), TtsProvider::MacSay { voice: "Karen".into() });
    }

    /// Content is spoken by a cloud voice only when the registry marks its
    /// terms safe; otherwise by the system voice (review A I-2, review B
    /// m7). A stale `tts_content_provider = 'minimax'` from an older build
    /// sends nothing: MiniMax's terms are `unknown`.
    #[test]
    fn content_goes_to_a_cloud_voice_only_when_its_terms_are_safe() {
        use crate::ai::registry::DataTerms;
        let db = fresh_db_with_prefs();
        db.conn.lock().unwrap().execute("UPDATE user_preferences SET value='minimax' WHERE key='tts_content_provider'", []).unwrap();
        let r = TtsRouter::new(db);
        let content = |language: &str| TtsRequest { text: "Your 3pm with Kenny moved.".into(), mode: TtsMode::Content, language: language.into() };
        assert_eq!(r.select_provider(&content("yue")).unwrap(), TtsProvider::MacSay { voice: "Sin-ji".into() });
        assert_eq!(r.select_provider(&content("en")).unwrap(), TtsProvider::MacSay { voice: "Samantha".into() });
        for terms in [DataTerms::Unknown, DataTerms::NeedsBillingConfirmation] {
            let _registry = crate::ai::registry::test_override::with(|reg| reg.providers.iter_mut().find(|p| p.id == "minimax").unwrap().data_terms = terms);
            assert_eq!(r.select_provider(&content("en")).unwrap(), TtsProvider::MacSay { voice: "Samantha".into() }, "{terms:?}");
        }
        let _registry = crate::ai::registry::test_override::with(|reg| reg.providers.iter_mut().find(|p| p.id == "minimax").unwrap().data_terms = DataTerms::Safe);
        assert_eq!(r.select_provider(&content("en")).unwrap(), TtsProvider::Minimax);
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
