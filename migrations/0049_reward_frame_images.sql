-- Uploaded artwork is immutable media. Only server-generated local PNG paths
-- enter the catalog; clients cannot supply arbitrary remote or filesystem URLs.
ALTER TABLE reward_catalog ADD COLUMN image_url TEXT;
ALTER TABLE reward_catalog DROP CONSTRAINT reward_catalog_check;
ALTER TABLE reward_catalog ADD CONSTRAINT reward_catalog_decoration_check CHECK (
    (kind='title' AND style IS NULL AND image_url IS NULL)
    OR (kind='avatar_frame' AND style IS NOT NULL AND (
        (style IN ('line','double','corners') AND image_url IS NULL)
        OR (style='image' AND (image_url IS NOT NULL OR NOT active))
    ))
);
ALTER TABLE reward_catalog ADD CONSTRAINT reward_catalog_image_url_check CHECK (
    image_url IS NULL OR image_url ~ '^/media/reward-frames/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.png$'
);
