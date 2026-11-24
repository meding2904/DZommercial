ALTER TABLE categories ADD COLUMN plugin_id TEXT NOT NULL DEFAULT 'oils';

CREATE INDEX IF NOT EXISTS idx_categories_plugin ON categories(plugin_id);
