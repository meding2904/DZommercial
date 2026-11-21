package plugins

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"time"

	"azahar/internal/secrets"
)

func (e *Env) Settings(ctx context.Context, pluginID string) (map[string]string, error) {
	values, _, err := e.SettingsWithTime(ctx, pluginID)
	return values, err
}

func (e *Env) SettingsWithTime(ctx context.Context, pluginID string) (map[string]string, time.Time, error) {
	raw, updatedAt, err := e.Store.PluginSettings(ctx, pluginID)
	if errors.Is(err, sql.ErrNoRows) {
		return map[string]string{}, time.Time{}, nil
	}
	if err != nil {
		return nil, time.Time{}, err
	}
	box := secrets.New(e.SecretKey)
	plain, err := box.Decrypt(raw)
	if err != nil {
		return nil, time.Time{}, ErrUnreadable
	}
	var values map[string]string
	if err := json.Unmarshal(plain, &values); err != nil {
		return nil, time.Time{}, ErrUnreadable
	}
	if values == nil {
		values = map[string]string{}
	}
	return values, updatedAt, nil
}

func (e *Env) EncodeSettings(values map[string]string) (string, error) {
	if values == nil {
		values = map[string]string{}
	}
	plain, err := json.Marshal(values)
	if err != nil {
		return "", err
	}
	box := secrets.New(e.SecretKey)
	return box.Encrypt(plain)
}

// MergeSettings applies a submitted settings form to the stored values. A blank
// secret keeps the stored secret, so the browser never needs the old key back.
func MergeSettings(manifest Manifest, existing, incoming map[string]string) (map[string]string, map[string]string) {
	merged := make(map[string]string, len(existing))
	for key, value := range existing {
		merged[key] = value
	}
	fields := make(map[string]string)

	known := make(map[string]bool, len(manifest.Settings))
	for _, field := range manifest.Settings {
		known[field.Key] = true
		value, present := incoming[field.Key]
		value = strings.TrimSpace(value)

		if field.Type == "secret" && present && value == "" {
			present = false
		}
		if !present {
			if field.Required && strings.TrimSpace(merged[field.Key]) == "" {
				fields[field.Key] = "This setting is required."
			}
			continue
		}

		switch field.Type {
		case "text", "secret":
			max := field.MaxLength
			if max <= 0 {
				if field.Type == "secret" {
					max = 512
				} else {
					max = 300
				}
			}
			if len([]rune(value)) > max {
				fields[field.Key] = fmt.Sprintf("Keep this under %d characters.", max+1)
				continue
			}
		case "number":
			if _, err := strconv.ParseFloat(value, 64); err != nil {
				fields[field.Key] = "Enter a number."
				continue
			}
		case "boolean":
			if value != "true" && value != "false" {
				fields[field.Key] = "Choose true or false."
				continue
			}
		case "select":
			valid := false
			for _, option := range field.Options {
				if option.Value == value {
					valid = true
					break
				}
			}
			if !valid {
				fields[field.Key] = "Choose one of the listed options."
				continue
			}
		}
		merged[field.Key] = value
	}

	for key := range incoming {
		if !known[key] {
			fields[key] = "This plugin has no such setting."
		}
	}
	return merged, fields
}

// MaskSettings removes secret values from the response and reports which ones
// already hold a value. The secret itself never leaves the server.
func MaskSettings(manifest Manifest, values map[string]string) (map[string]string, []string) {
	masked := make(map[string]string, len(manifest.Settings))
	secretsSet := make([]string, 0)
	for _, field := range manifest.Settings {
		value := values[field.Key]
		if value == "" {
			value = field.Default
		}
		if field.Type == "secret" {
			if values[field.Key] != "" {
				secretsSet = append(secretsSet, field.Key)
			}
			masked[field.Key] = ""
			continue
		}
		masked[field.Key] = value
	}
	return masked, secretsSet
}

func SettingsComplete(manifest Manifest, values map[string]string) bool {
	for _, field := range manifest.Settings {
		if field.Required && strings.TrimSpace(values[field.Key]) == "" {
			return false
		}
	}
	return true
}
