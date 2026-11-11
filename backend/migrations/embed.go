package migrations

import "embed"

// FS holds the SQL migration files applied at startup.
//
//go:embed *.sql
var FS embed.FS
