package store

import (
	"context"
	"fmt"
	"time"
)

// PluginStates returns the persisted enable/disable flags. Plugins without a
// row are treated as enabled by the registry.
func (s *Store) PluginStates(ctx context.Context) (map[string]bool, error) {
	rows, err := s.db.QueryContext(ctx,
		`SELECT plugin_id, enabled FROM plugin_states`)
	if err != nil {
		return nil, fmt.Errorf("list plugin states: %w", err)
	}
	defer rows.Close()

	states := make(map[string]bool)
	for rows.Next() {
		var pluginID string
		var enabled int
		if err := rows.Scan(&pluginID, &enabled); err != nil {
			return nil, fmt.Errorf("scan plugin state: %w", err)
		}
		states[pluginID] = enabled != 0
	}
	return states, rows.Err()
}

func (s *Store) SetPluginState(ctx context.Context, q querier, pluginID string, enabled bool, now time.Time) error {
	value := 0
	if enabled {
		value = 1
	}
	_, err := q.ExecContext(ctx,
		`INSERT INTO plugin_states (plugin_id, enabled, updated_at) VALUES (?, ?, ?)
		 ON CONFLICT(plugin_id) DO UPDATE SET
		   enabled = excluded.enabled,
		   updated_at = excluded.updated_at`,
		pluginID, value, FormatTime(now),
	)
	if err != nil {
		return fmt.Errorf("save plugin state: %w", err)
	}
	return nil
}
