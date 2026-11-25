ALTER TABLE order_items ADD COLUMN plugin_id TEXT NOT NULL DEFAULT 'core';
CREATE INDEX IF NOT EXISTS idx_order_items_plugin ON order_items(plugin_id);
