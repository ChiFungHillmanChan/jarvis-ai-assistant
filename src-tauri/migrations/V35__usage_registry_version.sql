-- Each usage row records the model registry version (P05) whose prices it
-- was costed at, so a later price change never makes an old row look wrong
-- or a new one look like an old one. 0 means before the app read a
-- registry: the row was costed with the price list built into that release
-- (and corrected once by ai/cost.rs correct_old_rows). A registry version is
-- 1 or higher (registry/schema.json).
--
-- inference_metrics gains the column; every existing request is 0.
ALTER TABLE inference_metrics ADD COLUMN registry_version INTEGER NOT NULL DEFAULT 0;

-- usage_daily keeps it too, as part of the key, so a day that spans a
-- registry update keeps one total per version. Rebuilt because a WITHOUT
-- ROWID table cannot change its primary key; the columns and the checks
-- are V34's, and every existing total is 0.
CREATE TABLE usage_daily_v35 (
    day INTEGER NOT NULL,
    provider TEXT NOT NULL,
    model_id TEXT NOT NULL,
    feature TEXT NOT NULL,
    price_unknown INTEGER NOT NULL CHECK (price_unknown IN (0, 1)),
    requests INTEGER NOT NULL CHECK (requests > 0),
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL,
    cache_write_tokens INTEGER NOT NULL,
    usd_cost_micro INTEGER,
    registry_version INTEGER NOT NULL DEFAULT 0,
    CHECK ((price_unknown = 0) = (usd_cost_micro IS NOT NULL)),
    PRIMARY KEY (day, provider, model_id, feature, price_unknown, registry_version)
) WITHOUT ROWID;

INSERT INTO usage_daily_v35
    (day, provider, model_id, feature, price_unknown, requests, input_tokens,
     output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro, registry_version)
SELECT day, provider, model_id, feature, price_unknown, requests, input_tokens,
       output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro, 0
FROM usage_daily;

DROP TABLE usage_daily;
ALTER TABLE usage_daily_v35 RENAME TO usage_daily;
