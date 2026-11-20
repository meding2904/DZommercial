package store

import (
	"context"
	"fmt"
)

const (
	defaultAuditFlushLimit = 200
	maxAuditFlushLimit     = 1000
)

// FlushAuditBatch copies one batch of queued audit rows into audit_log and
// removes them from audit_outbox in the same transaction.
func (s *Store) FlushAuditBatch(ctx context.Context, limit int) (int, error) {
	if limit <= 0 {
		limit = defaultAuditFlushLimit
	}
	if limit > maxAuditFlushLimit {
		limit = maxAuditFlushLimit
	}

	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return 0, fmt.Errorf("begin audit flush: %w", err)
	}
	defer tx.Rollback()

	// Audit ids are unique. If a row is already in audit_log, the entry is
	// present and the outbox copy is safe to remove.
	if _, err := tx.ExecContext(ctx,
		`INSERT OR IGNORE INTO audit_log (id, actor_user_id, action, entity_type, entity_id, detail, created_at)
		 SELECT id, actor_user_id, action, entity_type, entity_id, detail, created_at
		 FROM audit_outbox
		 ORDER BY created_at, id
		 LIMIT ?`, limit); err != nil {
		return 0, fmt.Errorf("insert audit batch: %w", err)
	}

	result, err := tx.ExecContext(ctx,
		`DELETE FROM audit_outbox
		 WHERE id IN (SELECT id FROM audit_outbox ORDER BY created_at, id LIMIT ?)`, limit)
	if err != nil {
		return 0, fmt.Errorf("delete audit batch: %w", err)
	}
	deleted, err := result.RowsAffected()
	if err != nil {
		return 0, fmt.Errorf("count audit batch: %w", err)
	}

	if err := tx.Commit(); err != nil {
		return 0, fmt.Errorf("commit audit flush: %w", err)
	}
	return int(deleted), nil
}
