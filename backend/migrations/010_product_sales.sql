ALTER TABLE products ADD COLUMN sale_price_cents INTEGER;
ALTER TABLE products ADD COLUMN sale_starts_at TEXT NOT NULL DEFAULT '';
ALTER TABLE products ADD COLUMN sale_ends_at TEXT NOT NULL DEFAULT '';
