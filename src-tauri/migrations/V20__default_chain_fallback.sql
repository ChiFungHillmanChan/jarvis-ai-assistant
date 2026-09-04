-- Rewrites only chains that are byte-for-byte the pre-fix gemini_primary default,
-- persisted by Settings.tsx writing get_provider_chain's response back verbatim.
-- The count and position-0 guards exist so a user who deliberately added Gemini
-- Flash as position 1 of their own chain keeps the cheap model they chose.
UPDATE provider_chain
   SET model_id = 'gemini-3.1-pro-preview'
 WHERE position = 1
   AND provider_type = 'gemini'
   AND model_id = 'gemini-3-flash-preview'
   AND (SELECT COUNT(*) FROM provider_chain) = 2
   AND EXISTS (
         SELECT 1 FROM provider_chain
          WHERE position = 0 AND provider_type = 'gemini' AND model_id IS NULL
       );
