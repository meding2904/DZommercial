package auditbatch

import (
	"context"
	"log/slog"
	"time"

	"azahar/internal/store"
)

const (
	flushLimit        = 200
	finalFlushTimeout = 5 * time.Second
)

// Run flushes queued audit rows immediately and then once per interval until
// ctx is cancelled. A final flush runs with its own timeout before returning.
func Run(ctx context.Context, st *store.Store, interval time.Duration, logger *slog.Logger) {
	if logger == nil {
		logger = slog.Default()
	}
	if interval <= 0 {
		interval = 2 * time.Second
	}

	flush := func(flushCtx context.Context) (int, error) {
		flushed, err := st.FlushAuditBatch(flushCtx, flushLimit)
		if err != nil {
			logger.Warn("audit batch flush failed; queued rows kept for the next tick", "error", err)
			return 0, err
		}
		if flushed > 0 {
			logger.Info("audit batch flushed", "rows", flushed)
		}
		return flushed, nil
	}

	for ctx.Err() == nil {
		flushed, err := flush(ctx)
		if err != nil || flushed < flushLimit {
			break
		}
	}

	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			finalCtx, cancel := context.WithTimeout(context.Background(), finalFlushTimeout)
			flush(finalCtx)
			cancel()
			return
		case <-ticker.C:
			flush(ctx)
		}
	}
}
