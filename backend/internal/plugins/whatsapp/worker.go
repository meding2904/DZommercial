package whatsapp

import (
	"context"
	"encoding/json"
	"errors"
	"time"
)

func (p *Plugin) run(ctx context.Context) {
	defer close(p.done)
	ticker := time.NewTicker(dispatchInterval)
	defer ticker.Stop()

	p.dispatch(ctx)
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			p.refreshConfig(ctx)
			p.dispatch(ctx)
		case <-p.wake:
			p.dispatch(ctx)
		}
	}
}

func (p *Plugin) triggerDispatch() {
	select {
	case p.wake <- struct{}{}:
	default:
	}
}

func (p *Plugin) dispatch(ctx context.Context) {
	env := p.envOrNil()
	if env == nil || env.DB == nil {
		return
	}
	config := p.currentConfig()
	if !config.active() {
		return
	}

	at := now().UTC()
	_, _ = env.DB.ExecContext(ctx,
		`UPDATE whatsapp_messages SET status = 'queued', next_attempt_at = ?, updated_at = ?
		 WHERE direction = 'out' AND status = 'sending' AND updated_at < ?`,
		timestamp(at), timestamp(at), timestamp(at.Add(-staleSendingAfter)),
	)

	rows, err := env.DB.QueryContext(ctx,
		`SELECT id, phone, kind, body, template_name, template_language, template_params, attempts
		 FROM whatsapp_messages
		 WHERE direction = 'out' AND status = 'queued'
		   AND (next_attempt_at = '' OR next_attempt_at <= ?)
		 ORDER BY created_at
		 LIMIT ?`,
		timestamp(at), defaultDispatchBatchSize,
	)
	if err != nil {
		p.warn("whatsapp queue could not be read", "error", err)
		return
	}
	queue := make([]queuedMessage, 0, defaultDispatchBatchSize)
	attempts := make(map[string]int, defaultDispatchBatchSize)
	for rows.Next() {
		var message queuedMessage
		var params string
		var count int
		if err := rows.Scan(&message.ID, &message.Phone, &message.Kind, &message.Body,
			&message.TemplateName, &message.TemplateLanguage, &params, &count); err != nil {
			rows.Close()
			p.warn("whatsapp queue row could not be read", "error", err)
			return
		}
		_ = json.Unmarshal([]byte(params), &message.TemplateParams)
		queue = append(queue, message)
		attempts[message.ID] = count
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		p.warn("whatsapp queue could not be read", "error", err)
		return
	}
	rows.Close()

	for _, message := range queue {
		if ctx.Err() != nil {
			return
		}
		optedOut, err := contactOptedOut(ctx, env.DB, message.Phone)
		if err != nil {
			p.warn("whatsapp consent check failed", "error", err)
			continue
		}
		if optedOut {
			_, _ = env.DB.ExecContext(ctx,
				`UPDATE whatsapp_messages SET status = 'cancelled',
				   error = 'The contact asked to stop WhatsApp messages.', updated_at = ?
				 WHERE id = ? AND status = 'queued'`,
				timestamp(now()), message.ID,
			)
			continue
		}
		if !p.claim(ctx, message.ID) {
			continue
		}
		metaID, sendErr := p.send(ctx, config, message)
		if sendErr != nil {
			p.retryOrFail(ctx, message.ID, attempts[message.ID]+1, sendErr)
			continue
		}
		if _, err := env.DB.ExecContext(ctx,
			`UPDATE whatsapp_messages SET status = 'sent', meta_message_id = ?, error = '',
			   updated_at = ? WHERE id = ?`,
			metaID, timestamp(now()), message.ID,
		); err != nil {
			p.warn("whatsapp sent state could not be saved", "error", err)
			continue
		}
		setState(ctx, env.DB, "last_sent_at", timestamp(now()))
	}
}

func (p *Plugin) claim(ctx context.Context, id string) bool {
	env := p.envOrNil()
	if env == nil || env.DB == nil {
		return false
	}
	at := timestamp(now())
	result, err := env.DB.ExecContext(ctx,
		`UPDATE whatsapp_messages SET status = 'sending', updated_at = ?
		 WHERE id = ? AND status = 'queued'`,
		at, id,
	)
	if err != nil {
		p.warn("whatsapp message could not be claimed", "error", err)
		return false
	}
	affected, err := result.RowsAffected()
	return err == nil && affected == 1
}

func (p *Plugin) retryOrFail(ctx context.Context, id string, attempts int, sendErr error) {
	env := p.envOrNil()
	if env == nil || env.DB == nil {
		return
	}
	permanent := false
	var apiError *sendError
	if errors.As(sendErr, &apiError) {
		permanent = apiError.Permanent
	}
	final := permanent || attempts >= maxSendAttempts
	status := "queued"
	nextAttempt := timestamp(now().UTC())
	if final {
		status = "failed"
		nextAttempt = ""
	} else {
		nextAttempt = timestamp(now().UTC().Add(retryBackoff(attempts)))
	}
	_, err := env.DB.ExecContext(ctx,
		`UPDATE whatsapp_messages SET status = ?, error = ?, attempts = ?,
		   next_attempt_at = ?, updated_at = ? WHERE id = ?`,
		status, safeError(sendErr), attempts, nextAttempt, timestamp(now()), id,
	)
	if err != nil {
		p.warn("whatsapp failure state could not be saved", "error", err)
		return
	}
	if final {
		setState(ctx, env.DB, "last_failure_at", timestamp(now()))
		setState(ctx, env.DB, "last_failure", safeError(sendErr))
	}
}

func retryBackoff(attempts int) time.Duration {
	delay := 30 * time.Second
	for index := 1; index < attempts; index++ {
		delay *= 2
		if delay >= 30*time.Minute {
			return 30 * time.Minute
		}
	}
	return delay
}

func safeError(err error) string {
	if err == nil {
		return ""
	}
	value := []rune(err.Error())
	if len(value) > 500 {
		value = value[:500]
	}
	return string(value)
}
