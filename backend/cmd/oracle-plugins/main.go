// Command oracle-plugins is a line-oriented differential oracle for the Rust
// port of internal/plugins.
//
// One JSON request per line, one JSON response per line. Registry scenarios
// carry small fake plugins and a list of steps; settings operations cover
// merge, mask, completeness, encryption and the plugin_settings table. The
// database connects lazily, so registry-only scenarios work without a server.
package main

import (
	"bufio"
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"sync"
	"time"

	"azahar/internal/db"
	"azahar/internal/plugins"
	"azahar/internal/secrets"
	"azahar/internal/store"
)

const defaultDSN = "http://127.0.0.1:8880"

type routeInput struct {
	Method  string `json:"method"`
	Pattern string `json:"pattern"`
	Admin   bool   `json:"admin"`
	Handler *bool  `json:"handler"`
}

type pluginInput struct {
	Manifest   plugins.Manifest    `json:"manifest"`
	Routes     []routeInput        `json:"routes"`
	Migrations []plugins.Migration `json:"migrations"`
}

type step struct {
	Op      string          `json:"op"`
	Index   int             `json:"index"`
	ID      string          `json:"id"`
	Enabled *bool           `json:"enabled"`
	States  map[string]bool `json:"states"`
}

type request struct {
	Op        string            `json:"op"`
	Plugins   []pluginInput     `json:"plugins"`
	Steps     []step            `json:"steps"`
	Manifest  *plugins.Manifest `json:"manifest"`
	Existing  map[string]string `json:"existing"`
	Incoming  map[string]string `json:"incoming"`
	Values    map[string]string `json:"values"`
	SecretKey string            `json:"secret_key"`
	PluginID  string            `json:"plugin_id"`
	Now       string            `json:"now"`
	Encrypted string            `json:"encrypted"`
}

type fakePlugin struct {
	manifest   plugins.Manifest
	routes     []plugins.Route
	migrations []plugins.Migration
}

func (p fakePlugin) Manifest() plugins.Manifest { return p.manifest }
func (p fakePlugin) Routes() []plugins.Route    { return p.routes }
func (p fakePlugin) Migrations() []plugins.Migration {
	return p.migrations
}

var databaseState struct {
	once   sync.Once
	handle *sql.DB
	repo   *store.Store
	err    error
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 64*1024), 8*1024*1024)
	writer := bufio.NewWriter(os.Stdout)
	defer writer.Flush()

	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}
		var req request
		if err := json.Unmarshal(line, &req); err != nil {
			writeJSON(writer, map[string]string{"error": "bad request: " + err.Error()})
			continue
		}
		handle(writer, req)
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "oracle-plugins: read:", err)
		os.Exit(1)
	}
}

func handle(writer *bufio.Writer, req request) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()

	switch req.Op {
	case "registry":
		writeJSON(writer, runRegistry(req))
	case "merge":
		manifest := manifestOrZero(req.Manifest)
		merged, fields := plugins.MergeSettings(manifest, req.Existing, req.Incoming)
		writeJSON(writer, map[string]any{"merged": merged, "fields": fields})
	case "mask":
		masked, set := plugins.MaskSettings(manifestOrZero(req.Manifest), req.Values)
		writeJSON(writer, map[string]any{"masked": masked, "secrets_set": set})
	case "complete":
		complete := plugins.SettingsComplete(manifestOrZero(req.Manifest), req.Values)
		writeJSON(writer, map[string]bool{"complete": complete})
	case "encode":
		env := &plugins.Env{SecretKey: req.SecretKey}
		encrypted, err := env.EncodeSettings(req.Values)
		if err != nil {
			writeJSON(writer, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(writer, map[string]string{"encrypted": encrypted})
	case "decode":
		box := secrets.New(req.SecretKey)
		plain, err := box.Decrypt(req.Encrypted)
		if err != nil {
			writeJSON(writer, map[string]string{"error": err.Error()})
			return
		}
		var values map[string]string
		if err := json.Unmarshal(plain, &values); err != nil {
			writeJSON(writer, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(writer, map[string]any{"values": values})
	case "save", "load", "delete":
		runSettings(ctx, writer, req)
	default:
		writeJSON(writer, map[string]string{"error": "unknown op"})
	}
}

func runRegistry(req request) map[string]any {
	registry := plugins.NewRegistry()
	results := make([]any, 0, len(req.Steps))

	for _, step := range req.Steps {
		switch step.Op {
		case "register":
			if step.Index < 0 || step.Index >= len(req.Plugins) {
				results = append(results, map[string]string{"error": "plugin index out of range"})
				continue
			}
			if err := registry.Register(buildPlugin(req.Plugins[step.Index])); err != nil {
				results = append(results, map[string]string{"error": err.Error()})
				continue
			}
			results = append(results, map[string]bool{"ok": true})
		case "validate":
			if err := registry.ValidateDependencies(); err != nil {
				results = append(results, map[string]string{"error": err.Error()})
				continue
			}
			results = append(results, map[string]bool{"ok": true})
		case "set_states":
			registry.SetStates(step.States)
			results = append(results, map[string]bool{"ok": true})
		case "set_enabled":
			enabled := false
			if step.Enabled != nil {
				enabled = *step.Enabled
			}
			results = append(results, map[string]bool{"result": registry.SetEnabled(step.ID, enabled)})
		case "enabled":
			results = append(results, map[string]bool{"result": registry.Enabled(step.ID)})
		case "effective_enabled":
			results = append(results, map[string]bool{"result": registry.EffectiveEnabled(step.ID)})
		case "dependents":
			results = append(results, map[string]any{"ids": registry.Dependents(step.ID)})
		case "enabled_dependents":
			results = append(results, map[string]any{"ids": registry.EnabledDependents(step.ID)})
		case "blocked_by":
			results = append(results, map[string]any{"ids": registry.BlockedBy(step.ID)})
		case "manifests":
			results = append(results, map[string]any{"manifests": registry.Manifests()})
		case "migrations":
			results = append(results, map[string]any{"migrations": registry.Migrations()})
		case "product_type":
			results = append(results, map[string]any{"product_type": registry.ProductType(step.ID)})
		case "all":
			all := registry.All()
			ids := make([]string, 0, len(all))
			for _, plugin := range all {
				ids = append(ids, plugin.Manifest().ID)
			}
			results = append(results, map[string]any{"ids": ids})
		case "get":
			_, ok := registry.Get(step.ID)
			results = append(results, map[string]bool{"found": ok})
		default:
			results = append(results, map[string]string{"error": "unknown step"})
		}
	}
	return map[string]any{"steps": results}
}

func buildPlugin(input pluginInput) plugins.Plugin {
	routes := make([]plugins.Route, 0, len(input.Routes))
	for _, route := range input.Routes {
		handler := func(*plugins.Context) {}
		if route.Handler != nil && !*route.Handler {
			handler = nil
		}
		routes = append(routes, plugins.Route{
			Method:  route.Method,
			Pattern: route.Pattern,
			Admin:   route.Admin,
			Handler: handler,
		})
	}
	return fakePlugin{
		manifest:   input.Manifest,
		routes:     routes,
		migrations: input.Migrations,
	}
}

func manifestOrZero(manifest *plugins.Manifest) plugins.Manifest {
	if manifest == nil {
		return plugins.Manifest{}
	}
	return *manifest
}

func runSettings(ctx context.Context, writer *bufio.Writer, req request) {
	handle, repo, err := database()
	if err != nil {
		writeJSON(writer, map[string]string{"error": err.Error()})
		return
	}
	env := &plugins.Env{Store: repo, SecretKey: req.SecretKey}

	switch req.Op {
	case "save":
		payload, err := env.EncodeSettings(req.Values)
		if err != nil {
			writeJSON(writer, map[string]string{"error": err.Error()})
			return
		}
		now := time.Now().UTC()
		if req.Now != "" {
			parsed, err := time.Parse(time.RFC3339, req.Now)
			if err != nil {
				writeJSON(writer, map[string]string{"error": "parse now: " + err.Error()})
				return
			}
			now = parsed.UTC()
		}
		if err := repo.SavePluginSettings(ctx, handle, req.PluginID, payload, now); err != nil {
			writeJSON(writer, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(writer, map[string]bool{"ok": true})
	case "load":
		values, updatedAt, err := env.SettingsWithTime(ctx, req.PluginID)
		if err != nil {
			writeJSON(writer, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(writer, map[string]any{
			"values":     values,
			"updated_at": store.FormatOptionalTime(updatedAt),
		})
	case "delete":
		if _, err := handle.ExecContext(ctx,
			"DELETE FROM plugin_settings WHERE plugin_id = ?", req.PluginID); err != nil {
			writeJSON(writer, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(writer, map[string]bool{"ok": true})
	}
}

func database() (*sql.DB, *store.Store, error) {
	databaseState.once.Do(func() {
		dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
		if dsn == "" {
			dsn = defaultDSN
		}
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		handle, err := db.Open(ctx, dsn)
		if err != nil {
			databaseState.err = fmt.Errorf(
				"open %s: %w (start the local libSQL server with make dev-db)", dsn, err)
			return
		}
		databaseState.handle = handle
		databaseState.repo = store.New(handle)
	})
	return databaseState.handle, databaseState.repo, databaseState.err
}

func writeJSON(writer *bufio.Writer, value any) {
	raw, err := json.Marshal(value)
	if err != nil {
		raw, err = json.Marshal(map[string]string{"error": err.Error()})
		if err != nil {
			fmt.Fprintln(os.Stderr, "oracle-plugins: encode:", err)
			os.Exit(1)
		}
	}
	writer.Write(raw)
	writer.WriteByte('\n')
	if err := writer.Flush(); err != nil {
		fmt.Fprintln(os.Stderr, "oracle-plugins: flush:", err)
		os.Exit(1)
	}
}
