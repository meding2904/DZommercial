package httpapi

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"reflect"
	"sort"
	"strings"
	"time"

	"azahar/internal/ids"
	"azahar/internal/plugins"
	"azahar/internal/store"
	"azahar/internal/storefront"

	"github.com/go-chi/chi/v5"
)

type publicPluginDTO struct {
	ID           string                   `json:"id"`
	Name         string                   `json:"name"`
	Version      string                   `json:"version"`
	Description  string                   `json:"description"`
	Author       string                   `json:"author,omitempty"`
	Dependencies []string                 `json:"dependencies,omitempty"`
	ProductType  *plugins.ProductType     `json:"product_type,omitempty"`
	Categories   []plugins.Category       `json:"categories,omitempty"`
	Pages        []plugins.StorefrontPage `json:"storefront_pages,omitempty"`
	AdminPath    string                   `json:"admin_path,omitempty"`
	Enabled      bool                     `json:"enabled"`
}

type adminPluginDTO struct {
	plugins.Manifest
	Configured       bool     `json:"configured"`
	SettingsState    string   `json:"settings_state"`
	UpdatedAt        string   `json:"updated_at,omitempty"`
	Enabled          bool     `json:"enabled"`
	EffectiveEnabled bool     `json:"effective_enabled"`
	Dependents       []string `json:"dependents,omitempty"`
	BlockedBy        []string `json:"blocked_by,omitempty"`
}

func (s *Server) handlePluginManifests(w http.ResponseWriter, r *http.Request) {
	result := make([]publicPluginDTO, 0)
	for _, plugin := range s.plugins.All() {
		manifest := plugin.Manifest()
		result = append(result, publicPluginDTO{
			ID:           manifest.ID,
			Name:         manifest.Name,
			Version:      manifest.Version,
			Description:  manifest.Description,
			Author:       manifest.Author,
			Dependencies: manifest.Dependencies,
			ProductType:  manifest.ProductType,
			Categories:   manifest.Categories,
			Pages:        manifest.StorefrontPages,
			AdminPath:    manifest.AdminPath,
			Enabled:      s.plugins.EffectiveEnabled(manifest.ID),
		})
	}
	writeJSON(w, http.StatusOK, map[string]any{"plugins": result})
}

func (s *Server) handleAdminPlugins(w http.ResponseWriter, r *http.Request) {
	result := make([]adminPluginDTO, 0)
	for _, plugin := range s.plugins.All() {
		result = append(result, s.adminPluginDTO(r.Context(), plugin))
	}
	writeJSON(w, http.StatusOK, map[string]any{"plugins": result})
}

func (s *Server) adminPluginDTO(ctx context.Context, plugin plugins.Plugin) adminPluginDTO {
	manifest := plugin.Manifest()
	dto := adminPluginDTO{
		Manifest:         manifest,
		SettingsState:    "ready",
		Enabled:          s.plugins.Enabled(manifest.ID),
		EffectiveEnabled: s.plugins.EffectiveEnabled(manifest.ID),
		Dependents:       s.plugins.Dependents(manifest.ID),
		BlockedBy:        s.plugins.BlockedBy(manifest.ID),
	}
	values, updatedAt, err := s.pluginSettings(ctx, manifest.ID)
	if err != nil {
		dto.SettingsState = "unreadable"
		return dto
	}
	dto.Configured = plugins.SettingsComplete(manifest, values)
	if !updatedAt.IsZero() {
		dto.UpdatedAt = store.FormatTime(updatedAt)
	}
	return dto
}

type pluginStatePayload struct {
	Enabled           *bool `json:"enabled"`
	ConfirmDependents bool  `json:"confirm_dependents"`
}

func (s *Server) handlePluginState(w http.ResponseWriter, r *http.Request) {
	plugin, ok := s.pluginFromPath(r)
	if !ok {
		writeError(w, http.StatusNotFound, "not_found", "Plugin not found.")
		return
	}
	payload, _, bad := decodeJSON[pluginStatePayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	if payload.Enabled == nil {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"enabled": "Choose whether the plugin is enabled."}))
		return
	}
	manifest := plugin.Manifest()
	enabled := *payload.Enabled

	if enabled {
		blocked := s.plugins.BlockedBy(manifest.ID)
		if len(blocked) > 0 {
			names := s.pluginNames(blocked)
			writeProblem(w, problemFields(409, "plugin_dependency",
				"Enable "+names+" before enabling "+manifest.Name+".",
				map[string]string{"dependencies": names}))
			return
		}
	} else {
		dependents := s.plugins.EnabledDependents(manifest.ID)
		if len(dependents) > 0 && !payload.ConfirmDependents {
			names := s.pluginNames(dependents)
			writeProblem(w, problemFields(409, "plugin_dependency_warning",
				"These enabled plugins depend on "+manifest.Name+": "+names+". They will stop working until it is enabled again.",
				map[string]string{"dependents": names}))
			return
		}
	}

	now := time.Now().UTC()
	actor, _ := currentAuth(r)
	detail, _ := json.Marshal(map[string]any{
		"enabled":      enabled,
		"dependencies": s.plugins.BlockedBy(manifest.ID),
		"dependents":   s.plugins.EnabledDependents(manifest.ID),
	})
	action := "plugin.disabled"
	if enabled {
		action = "plugin.enabled"
	}
	err := s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.SetPluginState(r.Context(), tx, manifest.ID, enabled, now); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      action,
			EntityType:  "plugin",
			EntityID:    manifest.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	s.plugins.SetEnabled(manifest.ID, enabled)
	writeJSON(w, http.StatusOK, map[string]any{
		"plugin": s.adminPluginDTO(r.Context(), plugin),
	})
}

func (s *Server) pluginNames(ids []string) string {
	names := make([]string, 0, len(ids))
	for _, id := range ids {
		if plugin, ok := s.plugins.Get(id); ok {
			names = append(names, plugin.Manifest().Name)
		} else {
			names = append(names, id)
		}
	}
	sort.Strings(names)
	return strings.Join(names, ", ")
}

func (s *Server) handleGetPluginSettings(w http.ResponseWriter, r *http.Request) {
	plugin, ok := s.pluginFromPath(r)
	if !ok {
		writeError(w, http.StatusNotFound, "not_found", "Plugin not found.")
		return
	}
	manifest := plugin.Manifest()
	values, updatedAt, err := s.pluginSettings(r.Context(), manifest.ID)
	if err != nil {
		writeError(w, http.StatusConflict, "plugin_settings_unreadable",
			"These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values.")
		return
	}
	masked, secretsSet := plugins.MaskSettings(manifest, values)
	response := map[string]any{
		"plugin_id":   manifest.ID,
		"settings":    masked,
		"secrets_set": secretsSet,
		"configured":  plugins.SettingsComplete(manifest, values),
	}
	if !updatedAt.IsZero() {
		response["updated_at"] = store.FormatTime(updatedAt)
	}
	writeJSON(w, http.StatusOK, response)
}

type pluginSettingsPayload struct {
	Settings map[string]string `json:"settings"`
}

func (s *Server) handlePutPluginSettings(w http.ResponseWriter, r *http.Request) {
	plugin, ok := s.pluginFromPath(r)
	if !ok {
		writeError(w, http.StatusNotFound, "not_found", "Plugin not found.")
		return
	}
	payload, _, bad := decodeJSON[pluginSettingsPayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	manifest := plugin.Manifest()

	existing, _, err := s.pluginSettings(r.Context(), manifest.ID)
	if err != nil && !errors.Is(err, plugins.ErrUnreadable) {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	if errors.Is(err, plugins.ErrUnreadable) {
		existing = map[string]string{}
		s.logger.Warn("plugin settings could not be decrypted; replacing them",
			"plugin", manifest.ID, "request_id", requestID(r))
	}

	merged, fields := plugins.MergeSettings(manifest, existing, payload.Settings)
	if validator, ok := plugin.(plugins.SettingsValidator); ok {
		for key, message := range validator.ValidateSettings(merged) {
			fields[key] = message
		}
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}
	if !plugins.SettingsComplete(manifest, merged) {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"settings": "Fill every required setting."}))
		return
	}

	sealed, err := s.pluginEnv.EncodeSettings(merged)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	now := time.Now().UTC()
	actor, _ := currentAuth(r)
	changed := changedSettingKeys(manifest, existing, merged)
	detail, _ := json.Marshal(map[string]any{"changed": changed})
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.SavePluginSettings(r.Context(), tx, manifest.ID, sealed, now); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "plugin.settings_update",
			EntityType:  "plugin",
			EntityID:    manifest.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	masked, secretsSet := plugins.MaskSettings(manifest, merged)
	writeJSON(w, http.StatusOK, map[string]any{
		"plugin_id":   manifest.ID,
		"settings":    masked,
		"secrets_set": secretsSet,
		"configured":  plugins.SettingsComplete(manifest, merged),
		"updated_at":  store.FormatTime(now),
	})
}

func (s *Server) pluginFromPath(r *http.Request) (plugins.Plugin, bool) {
	id := strings.TrimSpace(chi.URLParam(r, "pluginID"))
	if id == "" {
		return nil, false
	}
	return s.plugins.Get(id)
}

func (s *Server) pluginSettings(ctx context.Context, pluginID string) (map[string]string, time.Time, error) {
	return s.pluginEnv.SettingsWithTime(ctx, pluginID)
}

func changedSettingKeys(manifest plugins.Manifest, before, after map[string]string) []string {
	changed := make([]string, 0)
	for _, field := range manifest.Settings {
		if before[field.Key] != after[field.Key] {
			changed = append(changed, field.Key)
		}
	}
	sort.Strings(changed)
	return changed
}

func changedThemeFields(before, after storefront.Theme) []string {
	changed := make([]string, 0)
	walkThemeFields("", reflect.ValueOf(before), reflect.ValueOf(after), &changed)
	sort.Strings(changed)
	return changed
}

func walkThemeFields(prefix string, before, after reflect.Value, changed *[]string) {
	for index := 0; index < before.NumField(); index++ {
		name := before.Type().Field(index).Tag.Get("json")
		if comma := strings.Index(name, ","); comma >= 0 {
			name = name[:comma]
		}
		if name == "" || name == "-" || name == "updated_at" {
			continue
		}
		path := name
		if prefix != "" {
			path = prefix + "." + name
		}
		oldValue, newValue := before.Field(index), after.Field(index)
		if oldValue.Kind() == reflect.Struct {
			walkThemeFields(path, oldValue, newValue, changed)
			continue
		}
		if !reflect.DeepEqual(oldValue.Interface(), newValue.Interface()) {
			*changed = append(*changed, path)
		}
	}
}

func (s *Server) loadTheme(ctx context.Context) (storefront.Theme, error) {
	raw, updatedAt, err := s.store.StorefrontSettings(ctx)
	if errors.Is(err, sql.ErrNoRows) {
		return storefront.Default(), nil
	}
	if err != nil {
		return storefront.Theme{}, err
	}
	theme := storefront.ParseLenient(raw)
	theme.UpdatedAt = store.FormatTime(updatedAt)
	return theme, nil
}

func (s *Server) handleStorefrontTheme(w http.ResponseWriter, r *http.Request) {
	theme, err := s.loadTheme(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"theme": theme})
}

func (s *Server) handleAdminStorefrontTheme(w http.ResponseWriter, r *http.Request) {
	theme, err := s.loadTheme(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"theme":    theme,
		"defaults": storefront.Default(),
	})
}

func (s *Server) handlePutStorefrontTheme(w http.ResponseWriter, r *http.Request) {
	theme, _, bad := decodeJSON[storefront.Theme](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	theme = storefront.Normalize(theme)
	theme.UpdatedAt = ""
	if fields := storefront.Validate(theme); len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error",
			"Check the marked storefront settings.", fields))
		return
	}

	payload, err := storefront.Encode(theme)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	now := time.Now().UTC()
	actor, _ := currentAuth(r)
	previous, _ := s.loadTheme(r.Context())
	detail, _ := json.Marshal(map[string]any{"changed": changedThemeFields(previous, theme)})
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.SaveStorefrontSettings(r.Context(), tx, payload, now); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "storefront.theme_update",
			EntityType:  "storefront",
			EntityID:    "default",
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	theme.UpdatedAt = store.FormatTime(now)
	writeJSON(w, http.StatusOK, map[string]any{"theme": theme})
}
