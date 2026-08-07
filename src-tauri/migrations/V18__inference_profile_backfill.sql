-- Repair aws_credentials rows written before the geography fix.
--
-- set_aws_credentials used to store a hardcoded 'apac' whatever region the user
-- picked in Settings. The write path now derives the geography from the region,
-- and the stored value is genuinely consumed (ai/mod.rs rewrites the leading
-- geography segment of the Bedrock model id with it), so an install that
-- configured Bedrock on us-east-1 before the fix now sends an APAC inference
-- profile to a US endpoint and every volume-tier request fails with a
-- ValidationException. Re-pasting both AWS keys was the only recovery.
--
-- Bedrock geography prefixes are apac./us./eu.; unrecognised regions keep
-- 'apac', the column default in V13. This mapping must stay in step with
-- inference_profile_for_region in src/aws/credentials.rs.
UPDATE aws_credentials
SET inference_profile = CASE
        WHEN region LIKE 'us-%' THEN 'us'
        WHEN region LIKE 'eu-%' THEN 'eu'
        ELSE 'apac'
    END
WHERE inference_profile <> CASE
        WHEN region LIKE 'us-%' THEN 'us'
        WHEN region LIKE 'eu-%' THEN 'eu'
        ELSE 'apac'
    END;
