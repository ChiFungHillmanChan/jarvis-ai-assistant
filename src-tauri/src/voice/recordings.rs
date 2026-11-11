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

/// The WAV to keep for a voice request, or None when recordings are off, the
/// database is stored unencrypted or the audio cannot be encoded.
fn recording_to_keep(db: &Database, samples: &[f32]) -> Option<Vec<u8>> {
    if crate::db::stored_unencrypted() {
        return None;
    }
    let off = db.conn.lock().ok()?.query_row("SELECT value = 'false' FROM user_preferences WHERE key = ?1", [KEEP_PREFERENCE], |row| row.get(0));
    if off.unwrap_or(false) {
        return None;
    }
    wav(samples).map_err(|e| log::warn!("Voice recording not kept: {}", e)).ok()
}

/// Save a spoken request as the user's message in the voice conversation and
/// keep its recording. Returns the message to show in the chat and the
/// history to send to the AI. A recording that cannot be kept never fails the
/// request it came with.
pub fn save_spoken_turn(db: &Database, text: &str, samples: &[f32]) -> Result<(serde_json::Value, Vec<(String, String)>), String> {
    let recording = recording_to_keep(db, samples);
    let mut has_recording = false;
    let (id, history) = crate::commands::chat::save_turn_with(db, "legacy", text, |conn, id| {
        let Some(wav) = &recording else { return };
        match conn.execute("INSERT INTO voice_recordings (conversation_id, wav) VALUES (?1, ?2)", rusqlite::params![id, wav]) {
            Ok(_) => has_recording = true,
            Err(e) => log::warn!("Voice recording not kept: {}", e),
        }
    })?;
    Ok((serde_json::json!({ "role": "user", "content": text, "id": id, "has_recording": has_recording }), history))
}

/// The recording kept for a message, as a WAV file.
pub fn load(db: &Database, conversation_id: i64) -> Result<Option<Vec<u8>>, String> {
    use rusqlite::OptionalExtension;
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT wav FROM voice_recordings WHERE conversation_id = ?1", [conversation_id], |row| row.get(0)).optional().map_err(|e| e.to_string())
}

/// A voice setting was saved. Turning recordings off deletes the ones
/// already kept and gives their space in the database file back.
pub fn setting_changed(db: &Database, key: &str, value: &str) -> Result<(), String> {
    if key == KEEP_PREFERENCE && value == "false" {
        db.conn.lock().map_err(|e| e.to_string())?.execute("DELETE FROM voice_recordings", []).map_err(|e| e.to_string())?;
        // Deleted audio stays in the file's free pages until they are reclaimed.
        crate::commands::storage::reclaim_db(db);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_second() -> Vec<f32> {
        (0..16_000).map(|i| (i as f32 / 16_000.0 * 440.0 * std::f32::consts::TAU).sin() * 0.5).collect()
    }

    /// Say something, and get back the id of the message it was saved as.
    fn speak(db: &Database, samples: &[f32]) -> (i64, serde_json::Value) {
        let (shown, _) = save_spoken_turn(db, "what is on my calendar", samples).unwrap();
        (shown["id"].as_i64().expect("the chat needs the message id to fetch the recording"), shown)
    }

    fn turn_recordings_off(db: &Database) {
        db.conn.lock().unwrap().execute("INSERT INTO user_preferences (key, value) VALUES (?1, 'false')", [KEEP_PREFERENCE]).unwrap();
    }

    #[test]
    fn a_spoken_request_is_shown_in_the_chat_with_its_recording() {
        let db = crate::db::test_database();
        let (shown, history) = save_spoken_turn(&db, "what is on my calendar", &one_second()).unwrap();
        assert_eq!(history, [("user".to_string(), "what is on my calendar".to_string())]);
        assert_eq!((shown["role"].as_str(), shown["content"].as_str()), (Some("user"), Some("what is on my calendar")));
        assert_eq!(shown["has_recording"], true);
        assert!(load(&db, shown["id"].as_i64().unwrap()).unwrap().is_some());
    }

    #[test]
    fn a_kept_recording_plays_back_as_what_was_recorded() {
        let db = crate::db::test_database();
        let (message, _) = speak(&db, &one_second());

        let wav = load(&db, message).unwrap().expect("the recording was kept");
        let mut reader = hound::WavReader::new(std::io::Cursor::new(wav)).unwrap();
        assert_eq!((reader.spec().sample_rate, reader.spec().channels, reader.spec().bits_per_sample), (16_000, 1, 16));
        let played: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(played.len(), 16_000);
        assert_eq!(played[9], (one_second()[9] * 32767.0) as i16);
    }

    #[test]
    fn a_message_that_was_typed_has_no_recording() {
        let db = crate::db::test_database();
        let (typed, _) = crate::commands::chat::save_turn(&db, "legacy", "typed this one").unwrap();
        assert_eq!(load(&db, typed).unwrap(), None);
    }

    #[test]
    fn deleting_the_message_deletes_its_recording() {
        let db = crate::db::test_database();
        let (message, _) = speak(&db, &one_second());
        db.conn.lock().unwrap().execute("DELETE FROM conversations WHERE id = ?1", [message]).unwrap();
        let left: i64 = db.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM voice_recordings", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn nothing_is_kept_once_the_user_turns_recordings_off() {
        let db = crate::db::test_database();
        turn_recordings_off(&db);
        let (message, shown) = speak(&db, &one_second());
        assert_eq!(shown["has_recording"], false);
        assert_eq!(load(&db, message).unwrap(), None);
    }

    #[test]
    fn turning_recordings_off_deletes_the_ones_already_kept() {
        let db = crate::db::test_database();
        let (message, _) = speak(&db, &one_second());
        setting_changed(&db, "tts_enabled", "false").unwrap();
        setting_changed(&db, KEEP_PREFERENCE, "true").unwrap();
        assert!(load(&db, message).unwrap().is_some(), "only turning recordings off deletes them");
        setting_changed(&db, KEEP_PREFERENCE, "false").unwrap();
        assert_eq!(load(&db, message).unwrap(), None);
    }

    /// Deleted rows stay readable in the file's free pages until they are
    /// reclaimed, so "deleted" has to mean the file gives the space back.
    #[test]
    fn recordings_deleted_by_turning_them_off_leave_the_database_file() {
        let root = std::env::temp_dir().join(format!("jarvis-recordings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("jarvis.db");
        let size = |file: &std::path::Path| std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        {
            let db = crate::db::test_database_at(&path);
            let before = size(&path) + size(&root.join("jarvis.db-wal"));
            speak(&db, &vec![0.25; 16_000 * 30]);
            setting_changed(&db, KEEP_PREFERENCE, "false").unwrap();
            let after = size(&path) + size(&root.join("jarvis.db-wal"));
            assert!(after < before + 200_000, "a 960 KB recording was deleted but the file kept {} bytes more than before", after.saturating_sub(before));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Like mail and calendar content, a recording is not written to a
    /// database that could not be encrypted.
    #[test]
    fn nothing_is_kept_while_the_database_is_stored_unencrypted() {
        let db = crate::db::test_database();
        let (message, shown) = crate::db::TEST_STORED_UNENCRYPTED.sync_scope(true, || speak(&db, &one_second()));
        assert_eq!(shown["has_recording"], false);
        assert_eq!(load(&db, message).unwrap(), None);
    }

    /// Both voice paths and the settings command need a running app, so their
    /// calls into this module are checked in the source, like the wake-word guard.
    #[test]
    fn both_voice_paths_and_the_settings_command_go_through_this_module() {
        let push_to_talk = include_str!("commands.rs");
        assert!(push_to_talk.contains("recordings::save_spoken_turn(&db, &text, &samples)"), "push-to-talk must keep what was just transcribed");
        assert!(push_to_talk.contains("recordings::setting_changed(&db, &key, &value)"), "saving a voice setting must reach setting_changed");
        assert!(include_str!("wake_word.rs").contains("recordings::save_spoken_turn(&db, &user_text, &command_audio)"), "the wake-word request must keep what was just transcribed");
    }
}
