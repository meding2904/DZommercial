package whatsapp

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"time"
)

type execer interface {
	ExecContext(context.Context, string, ...any) (sql.Result, error)
	QueryRowContext(context.Context, string, ...any) *sql.Row
}

type outboundMessage struct {
	Phone            string
	ContactName      string
	Kind             string
	Body             string
	TemplateName     string
	TemplateLanguage string
	TemplateParams   []string
	OrderID          string
}

func enqueueMessage(ctx context.Context, q execer, message outboundMessage) error {
	at := timestamp(now())
	params, err := json.Marshal(message.TemplateParams)
	if err != nil {
		return fmt.Errorf("encode template parameters: %w", err)
	}
	_, err = q.ExecContext(ctx,
		`INSERT INTO whatsapp_messages (id, direction, kind, phone, contact_name, body,
		   template_name, template_language, template_params, status, error,
		   meta_message_id, order_id, attempts, next_attempt_at, created_at, updated_at)
		 VALUES (?, 'out', ?, ?, ?, ?, ?, ?, ?, 'queued', '', '', ?, 0, ?, ?, ?)`,
		newMessageID(), message.Kind, message.Phone, message.ContactName, message.Body,
		message.TemplateName, message.TemplateLanguage, string(params), message.OrderID,
		at, at, at,
	)
	if err != nil {
		return fmt.Errorf("insert outbound message: %w", err)
	}
	_, err = q.ExecContext(ctx,
		`INSERT INTO whatsapp_contacts (phone, name, opted_in, last_inbound_at, last_message_at, created_at, updated_at)
		 VALUES (?, ?, 1, '', ?, ?, ?)
		 ON CONFLICT(phone) DO UPDATE SET
		   name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE whatsapp_contacts.name END,
		   last_message_at = excluded.last_message_at,
		   updated_at = excluded.updated_at`,
		message.Phone, strings.TrimSpace(message.ContactName), at, at, at,
	)
	if err != nil {
		return fmt.Errorf("upsert outbound contact: %w", err)
	}
	return nil
}

func contactOptedOut(ctx context.Context, q execer, phone string) (bool, error) {
	var optedIn int
	err := q.QueryRowContext(ctx,
		`SELECT opted_in FROM whatsapp_contacts WHERE phone = ?`, phone,
	).Scan(&optedIn)
	if errors.Is(err, sql.ErrNoRows) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	return optedIn == 0, nil
}

func setState(ctx context.Context, q execer, key, value string) {
	_, _ = q.ExecContext(ctx,
		`INSERT INTO whatsapp_state (key, value, updated_at) VALUES (?, ?, ?)
		 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at`,
		key, value, timestamp(now()),
	)
}

func getState(ctx context.Context, q interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
}, key string) string {
	var value string
	if err := q.QueryRowContext(ctx,
		`SELECT value FROM whatsapp_state WHERE key = ?`, key).Scan(&value); err != nil {
		return ""
	}
	return value
}

func touchContact(ctx context.Context, q execer, phone, name, at string, optedIn *bool) error {
	name = strings.TrimSpace(name)
	if optedIn == nil {
		_, err := q.ExecContext(ctx,
			`INSERT INTO whatsapp_contacts (phone, name, opted_in, last_inbound_at, last_message_at, created_at, updated_at)
			 VALUES (?, ?, 1, ?, ?, ?, ?)
			 ON CONFLICT(phone) DO UPDATE SET
			   name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE whatsapp_contacts.name END,
			   last_inbound_at = CASE WHEN excluded.last_inbound_at > whatsapp_contacts.last_inbound_at
			     THEN excluded.last_inbound_at ELSE whatsapp_contacts.last_inbound_at END,
			   last_message_at = CASE WHEN excluded.last_message_at > whatsapp_contacts.last_message_at
			     THEN excluded.last_message_at ELSE whatsapp_contacts.last_message_at END,
			   updated_at = excluded.updated_at`,
			phone, name, at, at, at, at,
		)
		return err
	}
	value := 0
	if *optedIn {
		value = 1
	}
	_, err := q.ExecContext(ctx,
		`INSERT INTO whatsapp_contacts (phone, name, opted_in, last_inbound_at, last_message_at, created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?)
		 ON CONFLICT(phone) DO UPDATE SET
		   name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE whatsapp_contacts.name END,
		   opted_in = excluded.opted_in,
		   last_inbound_at = CASE WHEN excluded.last_inbound_at > whatsapp_contacts.last_inbound_at
		     THEN excluded.last_inbound_at ELSE whatsapp_contacts.last_inbound_at END,
		   last_message_at = CASE WHEN excluded.last_message_at > whatsapp_contacts.last_message_at
		     THEN excluded.last_message_at ELSE whatsapp_contacts.last_message_at END,
		   updated_at = excluded.updated_at`,
		phone, name, value, at, at, at, at,
	)
	return err
}

func inboundTimestamp(raw string) time.Time {
	seconds, err := parseUnix(raw)
	if err != nil {
		return now().UTC()
	}
	return time.Unix(seconds, 0).UTC()
}

func parseUnix(raw string) (int64, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return 0, errors.New("empty timestamp")
	}
	return strconv.ParseInt(raw, 10, 64)
}

func recordInbound(ctx context.Context, q execer, message inboundMessage) error {
	params, _ := json.Marshal([]string{})
	_, err := q.ExecContext(ctx,
		`INSERT OR IGNORE INTO whatsapp_messages (id, direction, kind, phone, contact_name, body,
		   template_name, template_language, template_params, status, error,
		   meta_message_id, order_id, attempts, next_attempt_at, created_at, updated_at)
		 VALUES (?, 'in', ?, ?, ?, ?, '', '', ?, 'received', '', ?, '', 0, '', ?, ?)`,
		newMessageID(), message.Kind, message.Phone, message.ContactName, message.Body,
		string(params), message.MetaID, message.At, message.At,
	)
	if err != nil {
		return fmt.Errorf("insert inbound message: %w", err)
	}
	optedIn, explicit := optInState(message.Body)
	var pointer *bool
	if explicit {
		pointer = &optedIn
	}
	if err := touchContact(ctx, q, message.Phone, message.ContactName, message.At, pointer); err != nil {
		return fmt.Errorf("touch inbound contact: %w", err)
	}
	return nil
}

type inboundMessage struct {
	Phone       string
	ContactName string
	Kind        string
	Body        string
	MetaID      string
	At          string
}

func optInState(body string) (bool, bool) {
	normalized := strings.ToUpper(strings.Join(strings.Fields(body), " "))
	switch normalized {
	case "STOP", "STOP ALL", "UNSUBSCRIBE", "ARRET", "ARRÊT", "إلغاء":
		return false, true
	case "START", "UNSTOP", "REPRENDRE", "ابدأ":
		return true, true
	default:
		return true, false
	}
}

func updateStatus(ctx context.Context, q execer, metaID, status, errorText, at string) error {
	var current string
	var id string
	err := q.QueryRowContext(ctx,
		`SELECT id, status FROM whatsapp_messages WHERE meta_message_id = ? AND direction = 'out'`,
		metaID,
	).Scan(&id, &current)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if !statusSupersedes(current, status) {
		return nil
	}
	_, err = q.ExecContext(ctx,
		`UPDATE whatsapp_messages SET status = ?, error = ?, updated_at = ? WHERE id = ?`,
		status, errorText, at, id,
	)
	return err
}

func statusSupersedes(current, next string) bool {
	rank := map[string]int{
		"queued": 0, "sending": 1, "sent": 2, "delivered": 3, "read": 4,
	}
	if next == "failed" {
		return current == "queued" || current == "sending" || current == "sent"
	}
	currentRank, ok := rank[current]
	if !ok {
		// A failed row can still receive a late sent, delivered or read receipt.
		return current == "failed" && rank[next] >= 2
	}
	nextRank, nextOK := rank[next]
	return nextOK && nextRank > currentRank
}
