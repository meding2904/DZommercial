package store

import (
	"context"
	"database/sql"
	"fmt"
	"time"
)

func (s *Store) CreateSession(ctx context.Context, q querier, session Session) error {
	_, err := q.ExecContext(ctx,
		`INSERT INTO sessions (id, user_id, token_hash, user_agent, ip, created_at, expires_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?)`,
		session.ID, session.UserID, session.TokenHash, session.UserAgent,
		session.IP, FormatTime(session.CreatedAt), FormatTime(session.ExpiresAt),
	)
	if err != nil {
		return fmt.Errorf("create session: %w", err)
	}
	return nil
}

func (s *Store) SessionUserByHash(ctx context.Context, tokenHash string) (User, Session, bool, error) {
	var session Session
	var user User
	var createdAt, expiresAt, userCreatedAt, userUpdatedAt string
	err := s.db.QueryRowContext(ctx,
		`SELECT s.id, s.user_id, s.token_hash, s.user_agent, s.ip, s.created_at, s.expires_at,
		        u.id, u.name, u.email, u.password_hash, u.role, u.theme_preset, u.created_at, u.updated_at
		 FROM sessions s JOIN users u ON u.id = s.user_id
		 WHERE s.token_hash = ?`, tokenHash,
	).Scan(
		&session.ID, &session.UserID, &session.TokenHash, &session.UserAgent,
		&session.IP, &createdAt, &expiresAt,
		&user.ID, &user.Name, &user.Email, &user.PasswordHash, &user.Role,
		&user.ThemePreset, &userCreatedAt, &userUpdatedAt,
	)
	if err != nil {
		if err == sql.ErrNoRows {
			return User{}, Session{}, false, nil
		}
		return User{}, Session{}, false, fmt.Errorf("load session: %w", err)
	}
	session.CreatedAt = ParseTime(createdAt)
	session.ExpiresAt = ParseTime(expiresAt)
	user.CreatedAt = ParseTime(userCreatedAt)
	user.UpdatedAt = ParseTime(userUpdatedAt)
	return user, session, true, nil
}

func (s *Store) DeleteSessionByHash(ctx context.Context, q querier, tokenHash string) error {
	_, err := q.ExecContext(ctx, `DELETE FROM sessions WHERE token_hash = ?`, tokenHash)
	return err
}

func (s *Store) DeleteSessionsForUserAgent(ctx context.Context, q querier, userID, userAgent string) error {
	if userAgent == "" {
		return nil
	}
	_, err := q.ExecContext(ctx,
		`DELETE FROM sessions WHERE user_id = ? AND user_agent = ?`, userID, userAgent,
	)
	return err
}

func (s *Store) DeleteExpiredSessions(ctx context.Context, q querier, now time.Time) error {
	_, err := q.ExecContext(ctx, `DELETE FROM sessions WHERE expires_at < ?`, FormatTime(now))
	return err
}
