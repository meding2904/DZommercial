ALTER TABLE products ADD COLUMN plugin_id TEXT NOT NULL DEFAULT 'core';
ALTER TABLE products ADD COLUMN attributes TEXT NOT NULL DEFAULT '{}';
UPDATE products SET attributes = '{}' WHERE attributes = '';

ALTER TABLE order_items ADD COLUMN options TEXT NOT NULL DEFAULT '{}';
ALTER TABLE order_items ADD COLUMN options_label TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS storefront_settings (
    id TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS plugin_settings (
    plugin_id TEXT PRIMARY KEY,
    settings TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_products_plugin ON products(plugin_id);
