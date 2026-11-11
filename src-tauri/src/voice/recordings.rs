//! Recordings of the user's own voice requests, kept so they can be played
//! back from the chat.
//!
//! A recording is stored in the database beside the message it belongs to and
//! is deleted with it. No AI tool reads this table, and nothing here leaves
//! the Mac.

use crate::db::Database;

/// The preference behind Settings > Speech > keep recordings. On unless the
/// user turned it off.
pub const KEEP_PREFERENCE: &str = "keep_voice_recordings";

/// 16 kHz mono 16-bit WAV, the format the microphone is captured in.
pub fn wav(samples: &[f32]) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: 16000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut file = std::io::Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut file, spec).map_err(|e| format!("WAV writer error: {}", e))?;
    for &sample in samples {
        let s16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(s16).map_err(|e| format!("WAV write error: {}", e))?;
    }
    writer.finalize().map_err(|e| format!("WAV finalize error: {}", e))?;
    Ok(file.into_inner())
}

fn wanted(db: &Database) -> bool {
    let Ok(conn) = db.conn.lock() else { return false };
    let off = conn.query_row("SELECT value = 'false' FROM user_preferences WHERE key = ?1", [KEEP_PREFERENCE], |row| row.get(0));
    !off.unwrap_or(false)
}

/// Keep the audio of the voice request saved as message `conversation_id`.
/// Returns whether it was kept. A recording that cannot be kept never fails
/// the request it came with.
pub fn keep(db: &Database, conversation_id: i64, samples: &[f32]) -> bool {
    if crate::db::stored_unencrypted() || !wanted(db) {
        return false;
    }
    let kept = wav(samples).and_then(|wav| {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO voice_recordings (conversation_id, wav) VALUES (?1, ?2)", rusqlite::params![conversation_id, wav]).map_err(|e| e.to_string())
    });
    if let Err(e) = &kept {
        log::warn!("Voice recording not kept: {}", e);
    }
    kept.is_ok()
}

/// Save a spoken request as the user's message in the voice conversation and
/// keep its recording. Returns the message to show in the chat and the
/// history to send to the AI.
pub fn save_spoken_turn(db: &Database, text: &str, samples: &[f32]) -> Result<(serde_json::Value, Vec<(String, String)>), String> {
    let (id, history) = crate::commands::chat::save_turn(db, "legacy", text)?;
    let has_recording = keep(db, id, samples);
    Ok((serde_json::json!({ "role": "user", "content": text, "id": id, "has_recording": has_recording }), history))
}

/// The recording kept for a message, as a WAV file.
pub fn load(db: &Database, conversation_id: i64) -> Result<Option<Vec<u8>>, String> {
    use rusqlite::OptionalExtension;
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT wav FROM voice_recordings WHERE conversation_id = ?1", [conversation_id], |row| row.get(0)).optional().map_err(|e| e.to_string())
}

/// A voice setting was saved. Turning recordings off deletes the ones
/// already kept.
pub fn setting_changed(db: &Database, key: &str, value: &str) -> Result<(), String> {
    if key == KEEP_PREFERENCE && value == "false" {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM voice_recordings", []).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_voice_message(db: &Database) -> i64 {
        let conn = db.conn.lock().unwrap();
        conn.execute("INSERT INTO conversations (role, content, session_id) VALUES ('user', 'what is on my calendar', 'legacy')", []).unwrap();
        conn.last_insert_rowid()
    }

    fn one_second() -> Vec<f32> {
        (0..16_000).map(|i| (i as f32 / 16_000.0 * 440.0 * std::f32::consts::TAU).sin() * 0.5).collect()
    }

    #[test]
    fn a_kept_recording_plays_back_as_what_was_recorded() {
        let db = crate::db::test_database();
        let message = a_voice_message(&db);
        assert!(keep(&db, message, &one_second()));

        let wav = load(&db, message).unwrap().expect("the recording was kept");
        let mut reader = hound::WavReader::new(std::io::Cursor::new(wav)).unwrap();
        assert_eq!((reader.spec().sample_rate, reader.spec().channels, reader.spec().bits_per_sample), (16_000, 1, 16));
        let played: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(played.len(), 16_000);
        assert_eq!(played[9], (one_second()[9] * 32767.0) as i16);
    }

    #[test]
    fn a_spoken_request_is_shown_in_the_chat_with_its_recording() {
        let db = crate::db::test_database();
        let (shown, history) = save_spoken_turn(&db, "what is on my calendar", &one_second()).unwrap();
        assert_eq!(history, [("user".to_string(), "what is on my calendar".to_string())]);
        assert_eq!((shown["role"].as_str(), shown["content"].as_str()), (Some("user"), Some("what is on my calendar")));
        assert_eq!(shown["has_recording"], true);
        let id = shown["id"].as_i64().expect("the chat needs the message id to fetch the recording");
        assert!(load(&db, id).unwrap().is_some());
    }

    #[test]
    fn a_spoken_request_without_a_kept_recording_is_shown_without_one() {
        let db = crate::db::test_database();
        db.conn.lock().unwrap().execute("INSERT INTO user_preferences (key, value) VALUES (?1, 'false')", [KEEP_PREFERENCE]).unwrap();
        let (shown, _) = save_spoken_turn(&db, "what is on my calendar", &one_second()).unwrap();
        assert_eq!(shown["has_recording"], false);
    }

    #[test]
    fn a_message_that_was_typed_has_no_recording() {
        let db = crate::db::test_database();
        assert_eq!(load(&db, a_voice_message(&db)).unwrap(), None);
    }

    #[test]
    fn deleting_the_message_deletes_its_recording() {
        let db = crate::db::test_database();
        let message = a_voice_message(&db);
        assert!(keep(&db, message, &one_second()));
        db.conn.lock().unwrap().execute("DELETE FROM conversations WHERE id = ?1", [message]).unwrap();
        let left: i64 = db.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM voice_recordings", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn nothing_is_kept_once_the_user_turns_recordings_off() {
        let db = crate::db::test_database();
        db.conn.lock().unwrap().execute("INSERT INTO user_preferences (key, value) VALUES (?1, 'false')", [KEEP_PREFERENCE]).unwrap();
        let message = a_voice_message(&db);
        assert!(!keep(&db, message, &one_second()));
        assert_eq!(load(&db, message).unwrap(), None);
    }

    #[test]
    fn turning_recordings_off_deletes_the_ones_already_kept() {
        let db = crate::db::test_database();
        let message = a_voice_message(&db);
        assert!(keep(&db, message, &one_second()));
        setting_changed(&db, "tts_enabled", "false").unwrap();
        setting_changed(&db, KEEP_PREFERENCE, "true").unwrap();
        assert!(load(&db, message).unwrap().is_some(), "only turning recordings off deletes them");
        setting_changed(&db, KEEP_PREFERENCE, "false").unwrap();
        assert_eq!(load(&db, message).unwrap(), None);
    }

    /// Like mail and calendar content, a recording is not written to a
    /// database that could not be encrypted.
    #[test]
    fn nothing_is_kept_while_the_database_is_stored_unencrypted() {
        let db = crate::db::test_database();
        let message = a_voice_message(&db);
        assert!(!crate::db::TEST_STORED_UNENCRYPTED.sync_scope(true, || keep(&db, message, &one_second())));
        assert_eq!(load(&db, message).unwrap(), None);
    }
}
