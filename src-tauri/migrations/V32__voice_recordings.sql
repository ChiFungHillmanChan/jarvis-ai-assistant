-- The recording behind a voice request, so the user can play it back from the
-- chat. It lives and dies with its message.
CREATE TABLE voice_recordings (
    conversation_id INTEGER PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    wav BLOB NOT NULL
);
