CREATE TABLE IF NOT EXISTS product_images (
    id TEXT PRIMARY KEY,
    mime TEXT NOT NULL,
    bytes BLOB NOT NULL,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_product_images_created ON product_images(created_at);
