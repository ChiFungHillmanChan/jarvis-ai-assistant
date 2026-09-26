-- A sender often resends an announcement with a corrected date ("please
-- disregard the deadline stated in our previous email"), or replies later
-- restating it. Each message used to be triaged on its own, so every version
-- of the date stayed on the agenda.
--
-- Triage now shows the extractor the sender's existing follow-ups and links
-- new items to them (mail/store.rs save_triage).
--
-- corrects_earlier is set when gpt-6-luna quoted words from the message that
-- say it corrects an earlier email, and code found that quote in the text.
-- superseded_by points at the newer item that replaced this one, so the
-- reader can say why it closed and the user can restore it.
ALTER TABLE mail_triage ADD COLUMN corrects_earlier INTEGER NOT NULL DEFAULT 0 CHECK (corrects_earlier IN (0,1));
ALTER TABLE mail_items ADD COLUMN superseded_by INTEGER REFERENCES mail_items(id) ON DELETE SET NULL;

-- Mail triaged before this had no chance to link its dates. Put the messages
-- that can link back in the queue once: those whose sender address has more
-- than one message with a dated follow-up. The next triage run checks them
-- again, oldest first, with the sender's other follow-ups in view. Other mail
-- is left alone, because a second extraction pass can word or judge an item
-- differently. Items the user acted on (done, dismissed, reminded, in the
-- calendar) are kept by save_triage.
WITH dated AS (
    SELECT DISTINCT i.message_id,
        lower(CASE WHEN instr(m.sender, '<') > 0
            THEN substr(m.sender, instr(m.sender, '<') + 1, instr(m.sender, '>') - instr(m.sender, '<') - 1)
            ELSE trim(m.sender) END) AS address
    FROM mail_items i JOIN mail_messages m ON m.id = i.message_id
    WHERE i.due_date IS NOT NULL
)
DELETE FROM mail_triage WHERE message_id IN (
    SELECT message_id FROM dated
    WHERE address != '' AND address IN (SELECT address FROM dated GROUP BY address HAVING COUNT(*) > 1)
);
