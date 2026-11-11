package store

import (
	"context"
	"fmt"
)

func (s *Store) InsertAudit(ctx context.Context, q querier, entry AuditEntry) error {
	if entry.Detail == "" {
		entry.Detail = "{}"
	}
	_, err := q.ExecContext(ctx,
		`INSERT INTO audit_log (id, actor_user_id, action, entity_type, entity_id, detail, created_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?)`,
		entry.ID, entry.ActorUserID, entry.Action, entry.EntityType,
		entry.EntityID, entry.Detail, FormatTime(entry.CreatedAt),
	)
	if err != nil {
		return fmt.Errorf("write audit log: %w", err)
	}
	return nil
}
