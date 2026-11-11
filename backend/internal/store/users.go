package store

import (
	"context"
	"database/sql"
	"fmt"
	"time"
)

func (s *Store) CountUsers(ctx context.Context) (int, error) {
	var count int
	err := s.db.QueryRowContext(ctx, `SELECT COUNT(*) FROM users`).Scan(&count)
	return count, err
}

func (s *Store) CreateUser(ctx context.Context, q querier, user User) error {
	_, err := q.ExecContext(ctx,
		`INSERT INTO users (id, name, email, password_hash, role, theme_preset, created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
		user.ID, user.Name, user.Email, user.PasswordHash, user.Role,
		defaultPreset(user.ThemePreset), FormatTime(user.CreatedAt), FormatTime(user.UpdatedAt),
	)
	if err != nil {
		return fmt.Errorf("create user: %w", err)
	}
	return nil
}

func (s *Store) UserByEmail(ctx context.Context, email string) (User, error) {
	return s.scanUser(s.db.QueryRowContext(ctx,
		`SELECT id, name, email, password_hash, role, theme_preset, created_at, updated_at
		 FROM users WHERE email = ?`, email,
	))
}

func (s *Store) UserByID(ctx context.Context, id string) (User, error) {
	return s.scanUser(s.db.QueryRowContext(ctx,
		`SELECT id, name, email, password_hash, role, theme_preset, created_at, updated_at
		 FROM users WHERE id = ?`, id,
	))
}

func (s *Store) scanUser(row scanner) (User, error) {
	var user User
	var createdAt, updatedAt string
	err := row.Scan(&user.ID, &user.Name, &user.Email, &user.PasswordHash,
		&user.Role, &user.ThemePreset, &createdAt, &updatedAt)
	if err != nil {
		return User{}, err
	}
	user.CreatedAt = ParseTime(createdAt)
	user.UpdatedAt = ParseTime(updatedAt)
	return user, nil
}

func (s *Store) UpdateTheme(ctx context.Context, q querier, userID, preset string, now time.Time) error {
	result, err := q.ExecContext(ctx,
		`UPDATE users SET theme_preset = ?, updated_at = ? WHERE id = ?`,
		preset, FormatTime(now), userID,
	)
	if err != nil {
		return fmt.Errorf("update theme: %w", err)
	}
	affected, err := result.RowsAffected()
	if err != nil {
		return fmt.Errorf("update theme rows: %w", err)
	}
	if affected == 0 {
		return sql.ErrNoRows
	}
	return nil
}

func (s *Store) ListCustomers(ctx context.Context) ([]CustomerRow, error) {
	rows, err := s.db.QueryContext(ctx,
		`SELECT u.id, u.name, u.email, u.created_at,
		        COUNT(CASE WHEN o.id IS NOT NULL AND o.status <> 'cancelled' THEN 1 END),
		        COALESCE(SUM(CASE WHEN o.status <> 'cancelled' THEN o.total_cents ELSE 0 END), 0),
		        COALESCE(MAX(CASE WHEN o.status <> 'cancelled' THEN o.created_at END), '')
		 FROM users u
		 LEFT JOIN orders o ON o.user_id = u.id
		 WHERE u.role = 'customer'
		 GROUP BY u.id, u.name, u.email, u.created_at
		 ORDER BY u.created_at DESC, u.email ASC`,
	)
	if err != nil {
		return nil, fmt.Errorf("list customers: %w", err)
	}
	defer rows.Close()

	customers := make([]CustomerRow, 0)
	for rows.Next() {
		var row CustomerRow
		var createdAt, lastOrderAt string
		if err := rows.Scan(&row.ID, &row.Name, &row.Email, &createdAt,
			&row.OrdersCount, &row.TotalSpentCents, &lastOrderAt); err != nil {
			return nil, fmt.Errorf("scan customer: %w", err)
		}
		row.CreatedAt = ParseTime(createdAt)
		row.LastOrderAt = NullTime(lastOrderAt)
		customers = append(customers, row)
	}
	return customers, rows.Err()
}

func defaultPreset(preset string) string {
	if preset == "" {
		return "cellar"
	}
	return preset
}
