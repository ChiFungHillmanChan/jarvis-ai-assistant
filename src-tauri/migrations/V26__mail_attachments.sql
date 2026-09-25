-- Attachment metadata for the Mail page: a JSON array of
-- {filename, mime_type, size}. The files themselves are not downloaded.
-- NULL means the message was saved before attachments were recorded, which is
-- not the same as having none: the Mail page keeps its generic "open in Gmail"
-- note for those until a rescan saves them again (save_messages upserts).
ALTER TABLE mail_messages ADD COLUMN attachments TEXT;
