package store_test

import (
	"context"
	"os"
	"testing"
	"time"

	"azahar/internal/db"
	"azahar/internal/ids"
	"azahar/internal/store"
)

func TestAuditOutboxFlushAgainstLibSQL(t *testing.T) {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("set AZAHAR_TEST_DATABASE_URL to run the libSQL integration test")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	handle, err := db.Open(ctx, dsn)
	if err != nil {
		t.Fatalf("open database: %v", err)
	}
	t.Cleanup(func() { handle.Close() })
	if err := db.Migrate(ctx, handle); err != nil {
		t.Fatalf("migrate: %v", err)
	}

	repo := store.New(handle)
	auditID := ids.New("aud_test")
	actorID := ids.New("usr_test")
	t.Cleanup(func() {
		if _, err := handle.Exec(`DELETE FROM audit_outbox WHERE id = ?`, auditID); err != nil {
			t.Errorf("cleanup audit_outbox: %v", err)
		}
		if _, err := handle.Exec(`DELETE FROM audit_log WHERE id = ?`, auditID); err != nil {
			t.Errorf("cleanup audit_log: %v", err)
		}
	})

	entry := store.AuditEntry{
		ID:          auditID,
		ActorUserID: actorID,
		Action:      "test.audit_batch",
		EntityType:  "test",
		EntityID:    "entity-" + auditID,
		Detail:      `{"source":"integration"}`,
		CreatedAt:   time.Now().UTC(),
	}
	if err := repo.InsertAudit(ctx, handle, entry); err != nil {
		t.Fatalf("insert audit: %v", err)
	}

	var queued int
	if err := handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM audit_outbox WHERE id = ?`, auditID,
	).Scan(&queued); err != nil {
		t.Fatalf("count audit_outbox: %v", err)
	}
	if queued != 1 {
		t.Fatalf("expected the audit row in audit_outbox, got %d", queued)
	}

	var logged int
	if err := handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM audit_log WHERE id = ?`, auditID,
	).Scan(&logged); err != nil {
		t.Fatalf("count audit_log: %v", err)
	}
	if logged != 0 {
		t.Fatalf("audit row reached audit_log before the flush")
	}

	flushed := 0
	for attempt := 0; attempt < 10; attempt++ {
		count, err := repo.FlushAuditBatch(ctx, 200)
		if err != nil {
			t.Fatalf("flush audit batch: %v", err)
		}
		flushed += count
		if err := handle.QueryRowContext(ctx,
			`SELECT COUNT(*) FROM audit_log WHERE id = ?`, auditID,
		).Scan(&logged); err != nil {
			t.Fatalf("count flushed audit_log: %v", err)
		}
		if logged == 1 || count == 0 {
			break
		}
	}
	if flushed == 0 {
		t.Fatal("expected at least one flushed audit row")
	}
	if logged != 1 {
		t.Fatal("audit row was not moved into audit_log")
	}

	var actor, action, entityType, entityID, detail string
	if err := handle.QueryRowContext(ctx,
		`SELECT actor_user_id, action, entity_type, entity_id, detail FROM audit_log WHERE id = ?`, auditID,
	).Scan(&actor, &action, &entityType, &entityID, &detail); err != nil {
		t.Fatalf("load flushed audit row: %v", err)
	}
	if actor != actorID || action != entry.Action || entityType != entry.EntityType ||
		entityID != entry.EntityID || detail != entry.Detail {
		t.Fatalf("flushed audit row does not match: actor=%q action=%q type=%q id=%q detail=%q",
			actor, action, entityType, entityID, detail)
	}

	if err := handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM audit_outbox WHERE id = ?`, auditID,
	).Scan(&queued); err != nil {
		t.Fatalf("count flushed audit_outbox: %v", err)
	}
	if queued != 0 {
		t.Fatalf("expected the audit row to be removed from audit_outbox, got %d", queued)
	}
}
