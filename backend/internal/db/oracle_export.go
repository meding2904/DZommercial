//go:build oracle

// Oracle-only export of the migration splitter for differential tests.
package db

func OracleSplitStatements(body string) []string { return splitStatements(body) }
