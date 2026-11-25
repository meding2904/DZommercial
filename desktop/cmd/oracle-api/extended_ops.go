//go:build oracle

package main

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"azahar/desktop/internal/api"
)

// Settings, pure helpers and secret store scripts used by the extended
// differential test.

type storeSpec struct {
	Kind       string     `json:"kind"`
	Path       string     `json:"path"`
	Prefill    *string    `json:"prefill"`
	Value      *string    `json:"value"`
	Present    bool       `json:"present"`
	Fail       bool       `json:"fail"`
	DeleteFail bool       `json:"delete_fail"`
	SleepMS    int        `json:"sleep_ms"`
	TimeoutMS  int        `json:"timeout_ms"`
	Primary    *storeSpec `json:"primary"`
	Fallback   *storeSpec `json:"fallback"`
	Inner      *storeSpec `json:"inner"`
}

type storeOp struct {
	Kind  string `json:"kind"`
	Value string `json:"value"`
}

type storeOpResult struct {
	OK      bool    `json:"ok"`
	Value   *string `json:"value"`
	Present bool    `json:"present"`
	Plain   string  `json:"plain,omitempty"`
}

type builtStore struct {
	store  api.OracleSecretStore
	memory *scriptMemory
	path   string
}

func buildStore(spec *storeSpec, root string) builtStore {
	if spec == nil {
		return builtStore{}
	}
	switch spec.Kind {
	case "file":
		relative := spec.Path
		if relative == "" {
			relative = "session.json"
		}
		full := filepath.Join(root, relative)
		if spec.Prefill != nil {
			_ = os.MkdirAll(filepath.Dir(full), 0o700)
			_ = os.WriteFile(full, []byte(*spec.Prefill), 0o644)
		}
		return builtStore{store: api.OracleFileStore(full), path: full}
	case "memory":
		memory := &scriptMemory{
			fail:       spec.Fail,
			deleteFail: spec.DeleteFail,
			sleep:      time.Duration(spec.SleepMS) * time.Millisecond,
		}
		if spec.Value != nil {
			memory.value = *spec.Value
			memory.present = true
		}
		return builtStore{store: memory, memory: memory}
	case "slow":
		memory := &scriptMemory{sleep: time.Duration(spec.SleepMS) * time.Millisecond}
		return builtStore{store: memory, memory: memory}
	case "fallback":
		primary := buildStore(spec.Primary, root)
		fallback := buildStore(spec.Fallback, root)
		return builtStore{
			store:  api.OracleFallbackStore(primary.store, fallback.store),
			memory: primary.memory,
			path:   fallback.path,
		}
	case "timeout":
		inner := buildStore(spec.Inner, root)
		return builtStore{
			store:  api.OracleTimeoutStore(inner.store, time.Duration(spec.TimeoutMS)*time.Millisecond),
			memory: inner.memory,
			path:   inner.path,
		}
	default:
		return builtStore{}
	}
}

func runSecretOps(cmd command) result {
	root, err := os.MkdirTemp("", "oracle-secret-")
	if err != nil {
		return wrap(nil, &api.Error{Code: "oracle_temp", Message: err.Error(), Fields: map[string]string{}})
	}
	defer os.RemoveAll(root)

	built := buildStore(cmd.Store, root)
	if built.store == nil {
		return wrap(nil, &api.Error{Code: "oracle_store", Message: "unknown store", Fields: map[string]string{}})
	}

	results := make([]storeOpResult, 0, len(cmd.Ops))
	for _, op := range cmd.Ops {
		switch op.Kind {
		case "load":
			value, present, loadErr := built.store.Load()
			if loadErr != nil {
				results = append(results, storeOpResult{OK: false, Plain: loadErr.Error()})
				continue
			}
			results = append(results, storeOpResult{OK: true, Value: &value, Present: present})
		case "save":
			if saveErr := built.store.Save(op.Value); saveErr != nil {
				results = append(results, storeOpResult{OK: false, Plain: saveErr.Error()})
				continue
			}
			results = append(results, storeOpResult{OK: true})
		case "delete":
			if deleteErr := built.store.Delete(); deleteErr != nil {
				results = append(results, storeOpResult{OK: false, Plain: deleteErr.Error()})
				continue
			}
			results = append(results, storeOpResult{OK: true})
		case "state":
			if built.memory == nil {
				results = append(results, storeOpResult{OK: false, Plain: "no memory state"})
				continue
			}
			snapshot := built.memory.snapshot()
			results = append(results, storeOpResult{OK: true, Value: &snapshot.Value, Present: snapshot.Present})
		default:
			results = append(results, storeOpResult{OK: false, Plain: "unknown op " + op.Kind})
		}
	}

	value := map[string]any{"ops": results, "path": nil}
	if built.path != "" {
		value["path"] = snapshotPath(built.path)
	}
	return wrap(value, nil)
}

func runSettingsLoad(cmd command) result {
	root, err := os.MkdirTemp("", "oracle-settings-")
	if err != nil {
		return wrap(nil, &api.Error{Code: "oracle_temp", Message: err.Error(), Fields: map[string]string{}})
	}
	defer os.RemoveAll(root)
	profile := filepath.Join(root, "profile")

	env := isolatedEnv(root)
	for name, value := range cmd.Env {
		env[name] = value
	}
	if cmd.SettingsRaw != nil {
		_ = os.MkdirAll(profile, 0o700)
		_ = os.WriteFile(filepath.Join(profile, "settings.json"), []byte(*cmd.SettingsRaw), 0o644)
	}
	if cmd.LegacyRaw != nil {
		for name, value := range prepareLegacy(root, cmd.LegacyKind, *cmd.LegacyRaw) {
			env[name] = value
		}
	}

	restore := applyEnv(env)
	client, clientErr := api.OracleNewClientLoaded("2.0.0-test", profile, &scriptMemory{})
	restore()
	if clientErr != nil {
		return wrap(nil, &api.Error{Code: "oracle_client", Message: clientErr.Error(), Fields: map[string]string{}})
	}
	return wrap(map[string]any{
		"settings": api.OracleSettingsSnapshot(client),
		"info":     client.Info(),
		"window":   client.WindowBounds(),
		"profile":  snapshotProfile(profile),
	}, nil)
}

func runSaveSettings(cmd command) result {
	root, err := os.MkdirTemp("", "oracle-save-")
	if err != nil {
		return wrap(nil, &api.Error{Code: "oracle_temp", Message: err.Error(), Fields: map[string]string{}})
	}
	defer os.RemoveAll(root)
	settings := api.Settings{}
	if cmd.Settings != nil {
		settings = *cmd.Settings
	}
	saveErr := api.OracleSaveSettingsFile(filepath.Join(root, "settings.json"), settings)
	return wrap(map[string]any{
		"failed":  saveErr != nil,
		"path":    snapshotPath(filepath.Join(root, "settings.json")),
		"profile": snapshotProfile(root),
	}, nil)
}

func writeLeftovers(path string) []string {
	dir := filepath.Dir(path)
	base := filepath.Base(path)
	entries, err := os.ReadDir(dir)
	if err != nil {
		return []string{}
	}
	leftovers := []string{}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), "."+base+".") {
			leftovers = append(leftovers, "temp")
		}
	}
	sort.Strings(leftovers)
	return leftovers
}

func runWritePrivateFile(cmd command) result {
	root, err := os.MkdirTemp("", "oracle-private-")
	if err != nil {
		return wrap(nil, &api.Error{Code: "oracle_temp", Message: err.Error(), Fields: map[string]string{}})
	}
	defer os.RemoveAll(root)
	path := filepath.Join(root, cmd.Path)
	if cmd.BlockParent {
		_ = os.MkdirAll(filepath.Dir(path), 0o700)
		_ = os.WriteFile(filepath.Dir(path), []byte("blocked"), 0o600)
	}
	if cmd.Precreate != nil {
		_ = os.MkdirAll(filepath.Dir(path), 0o700)
		_ = os.WriteFile(path, []byte(*cmd.Precreate), 0o644)
	}
	writeErr := api.OracleWritePrivateFile(path, []byte(cmd.Data))
	return wrap(map[string]any{
		"failed":    writeErr != nil,
		"path":      snapshotPath(path),
		"parent":    snapshotPath(filepath.Dir(path)),
		"leftovers": writeLeftovers(path),
	}, nil)
}

func runLoadLegacy(cmd command) result {
	root, err := os.MkdirTemp("", "oracle-legacy-")
	if err != nil {
		return wrap(nil, &api.Error{Code: "oracle_temp", Message: err.Error(), Fields: map[string]string{}})
	}
	defer os.RemoveAll(root)

	env := isolatedEnv(root)
	if cmd.LegacyRaw != nil {
		for name, value := range prepareLegacy(root, cmd.LegacyKind, *cmd.LegacyRaw) {
			env[name] = value
		}
	}
	for name, value := range cmd.Env {
		env[name] = value
	}

	restore := applyEnv(env)
	settings, present := api.OracleLoadLegacySettings()
	restore()
	return wrap(map[string]any{"present": present, "settings": settings}, nil)
}

func runDecodeSetting(cmd command) result {
	value, applied := api.OracleDecodeSetting(cmd.Doc, cmd.Key, cmd.Kind)
	return wrap(map[string]any{"value": value, "applied": applied}, nil)
}

func runConsentHostForUpdate(cmd command) result {
	previous := api.Settings{}
	if cmd.Previous != nil {
		previous = *cmd.Previous
	}
	host, apiErr := api.OracleConsentHostForUpdate(previous, cmd.Raw, cmd.AllowNetwork)
	return wrap(host, apiErr)
}

func runSettingsConsentAllowed(cmd command) result {
	settings := api.Settings{}
	if cmd.Settings != nil {
		settings = *cmd.Settings
	}
	return wrap(api.OracleSettingsConsentAllowed(settings), nil)
}

func runDefaultSecretStoreKind(cmd command) result {
	restore := applyEnv(cmd.Env)
	kind := api.OracleDefaultSecretStoreKind(cmd.Profile, cmd.Raw)
	restore()
	return wrap(kind, nil)
}

func runEnvEnabled(cmd command) result {
	env := map[string]*string{}
	if cmd.ValuePresent {
		value := cmd.Value
		env["AZAHAR_ORACLE_ENV_ENABLED"] = &value
	} else {
		env["AZAHAR_ORACLE_ENV_ENABLED"] = nil
	}
	restore := applyEnv(env)
	enabled := api.OracleEnvEnabled("AZAHAR_ORACLE_ENV_ENABLED")
	restore()
	return wrap(enabled, nil)
}
