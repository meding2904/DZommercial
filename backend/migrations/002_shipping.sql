ALTER TABLE orders ADD COLUMN wilaya_code INTEGER;
ALTER TABLE orders ADD COLUMN wilaya_name TEXT NOT NULL DEFAULT '';
ALTER TABLE orders ADD COLUMN commune_name TEXT NOT NULL DEFAULT '';
ALTER TABLE orders ADD COLUMN delivery_type TEXT NOT NULL DEFAULT 'home';
ALTER TABLE orders ADD COLUMN stop_desk_id TEXT NOT NULL DEFAULT '';
ALTER TABLE orders ADD COLUMN stop_desk_name TEXT NOT NULL DEFAULT '';
ALTER TABLE orders ADD COLUMN courier_key TEXT NOT NULL DEFAULT '';
ALTER TABLE orders ADD COLUMN tracking_number TEXT NOT NULL DEFAULT '';
ALTER TABLE orders ADD COLUMN courier_status TEXT NOT NULL DEFAULT '';
ALTER TABLE orders ADD COLUMN courier_events TEXT NOT NULL DEFAULT '[]';
ALTER TABLE orders ADD COLUMN courier_synced_at TEXT NOT NULL DEFAULT '';

UPDATE orders SET wilaya_code = 16, wilaya_name = 'Alger', commune_name = 'Alger Centre', city = 'Alger Centre'
WHERE wilaya_code IS NULL AND city IN ('Algiers', 'Alger');
UPDATE orders SET wilaya_code = 31, wilaya_name = 'Oran', commune_name = 'Oran', city = 'Oran'
WHERE wilaya_code IS NULL AND city = 'Oran';
UPDATE orders SET wilaya_code = 25, wilaya_name = 'Constantine', commune_name = 'Constantine', city = 'Constantine'
WHERE wilaya_code IS NULL AND city = 'Constantine';
UPDATE orders SET wilaya_code = 13, wilaya_name = 'Tlemcen', commune_name = 'Tlemcen', city = 'Tlemcen'
WHERE wilaya_code IS NULL AND city = 'Tlemcen';
UPDATE orders SET wilaya_code = 23, wilaya_name = 'Annaba', commune_name = 'Annaba', city = 'Annaba'
WHERE wilaya_code IS NULL AND city = 'Annaba';
UPDATE orders SET wilaya_code = 9, wilaya_name = 'Blida', commune_name = 'Blida', city = 'Blida'
WHERE wilaya_code IS NULL AND city = 'Blida';
