package db

import (
	"context"
	"database/sql"
	"fmt"
	"sort"
	"strings"
	"time"
)

// PluginMigration is one SQL file owned by a plugin. The core records it in the
// same way as its own migrations, keyed by plugin id and version.
type PluginMigration struct {
	PluginID string
	Version  string
	SQL      string
}

func MigratePlugins(ctx context.Context, handle *sql.DB, list []PluginMigration) error {
	if _, err := handle.ExecContext(ctx, `CREATE TABLE IF NOT EXISTS plugin_migrations (
		plugin_id TEXT NOT NULL,
		version TEXT NOT NULL,
		applied_at TEXT NOT NULL,
		PRIMARY KEY (plugin_id, version)
	)`); err != nil {
		return fmt.Errorf("create plugin_migrations: %w", err)
	}

	migrations := append([]PluginMigration(nil), list...)
	sort.SliceStable(migrations, func(i, j int) bool {
		if migrations[i].PluginID != migrations[j].PluginID {
			return migrations[i].PluginID < migrations[j].PluginID
		}
		return migrations[i].Version < migrations[j].Version
	})

	seen := make(map[string]bool)
	for _, migration := range migrations {
		if strings.TrimSpace(migration.PluginID) == "" || strings.TrimSpace(migration.Version) == "" {
			return fmt.Errorf("plugin migration needs a plugin id and version")
		}
		key := migration.PluginID + ":" + migration.Version
		if seen[key] {
			return fmt.Errorf("duplicate plugin migration %s", key)
		}
		seen[key] = true

		var applied int
		if err := handle.QueryRowContext(ctx,
			`SELECT COUNT(*) FROM plugin_migrations WHERE plugin_id = ? AND version = ?`,
			migration.PluginID, migration.Version,
		).Scan(&applied); err != nil {
			return fmt.Errorf("check plugin migration %s: %w", key, err)
		}
		if applied > 0 {
			continue
		}

		tx, err := handle.BeginTx(ctx, nil)
		if err != nil {
			return fmt.Errorf("begin plugin migration %s: %w", key, err)
		}
		for _, statement := range splitStatements(migration.SQL) {
			if _, err := tx.ExecContext(ctx, statement); err != nil {
				tx.Rollback()
				return fmt.Errorf("apply plugin migration %s: %w", key, err)
			}
		}
		if _, err := tx.ExecContext(ctx,
			`INSERT INTO plugin_migrations (plugin_id, version, applied_at) VALUES (?, ?, ?)`,
			migration.PluginID, migration.Version, time.Now().UTC().Format(time.RFC3339),
		); err != nil {
			tx.Rollback()
			return fmt.Errorf("record plugin migration %s: %w", key, err)
		}
		if err := tx.Commit(); err != nil {
			return fmt.Errorf("commit plugin migration %s: %w", key, err)
		}
	}
	return nil
}
