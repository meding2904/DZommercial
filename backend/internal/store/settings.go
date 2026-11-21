package store

import (
	"context"
	"fmt"
	"time"
)

func (s *Store) PluginSettings(ctx context.Context, pluginID string) (string, time.Time, error) {
	var raw, updatedAt string
	err := s.db.QueryRowContext(ctx,
		`SELECT settings, updated_at FROM plugin_settings WHERE plugin_id = ?`, pluginID,
	).Scan(&raw, &updatedAt)
	if err != nil {
		return "", time.Time{}, err
	}
	return raw, ParseTime(updatedAt), nil
}

func (s *Store) SavePluginSettings(ctx context.Context, q querier, pluginID, payload string, now time.Time) error {
	_, err := q.ExecContext(ctx,
		`INSERT INTO plugin_settings (plugin_id, settings, updated_at) VALUES (?, ?, ?)
		 ON CONFLICT(plugin_id) DO UPDATE SET settings = excluded.settings, updated_at = excluded.updated_at`,
		pluginID, payload, FormatTime(now),
	)
	if err != nil {
		return fmt.Errorf("save plugin settings: %w", err)
	}
	return nil
}

func (s *Store) StorefrontSettings(ctx context.Context) (string, time.Time, error) {
	return s.storefrontSettings(ctx, s.db)
}

// StorefrontSettingsTx reads the theme inside a transaction so a writer can
// compare it with the preview baseline before saving.
func (s *Store) StorefrontSettingsTx(ctx context.Context, q querier) (string, time.Time, error) {
	return s.storefrontSettings(ctx, q)
}

func (s *Store) storefrontSettings(ctx context.Context, q querier) (string, time.Time, error) {
	var raw, updatedAt string
	err := q.QueryRowContext(ctx,
		`SELECT data, updated_at FROM storefront_settings WHERE id = 'default'`,
	).Scan(&raw, &updatedAt)
	if err != nil {
		return "", time.Time{}, err
	}
	return raw, ParseTime(updatedAt), nil
}

func (s *Store) SaveStorefrontSettings(ctx context.Context, q querier, payload string, now time.Time) error {
	_, err := q.ExecContext(ctx,
		`INSERT INTO storefront_settings (id, data, updated_at) VALUES ('default', ?, ?)
		 ON CONFLICT(id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at`,
		payload, FormatTime(now),
	)
	if err != nil {
		return fmt.Errorf("save storefront settings: %w", err)
	}
	return nil
}
