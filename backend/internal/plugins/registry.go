package plugins

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"sync"

	"azahar/internal/store"
)

type Registry struct {
	mu      sync.RWMutex
	order   []Plugin
	byID    map[string]Plugin
	enabled map[string]bool
}

func NewRegistry() *Registry {
	return &Registry{
		byID:    make(map[string]Plugin),
		enabled: make(map[string]bool),
	}
}

func (r *Registry) Register(plugin Plugin) error {
	if plugin == nil {
		return fmt.Errorf("register plugin: nil plugin")
	}
	manifest := plugin.Manifest()
	if !idPattern.MatchString(manifest.ID) {
		return fmt.Errorf("register plugin %q: id must be 2 to 32 lowercase letters, numbers, dashes or underscores", manifest.ID)
	}
	if _, exists := r.byID[manifest.ID]; exists {
		return fmt.Errorf("register plugin %q: already registered", manifest.ID)
	}
	if strings.TrimSpace(manifest.Name) == "" || len([]rune(manifest.Name)) > 80 {
		return fmt.Errorf("register plugin %q: name must be 1 to 80 characters", manifest.ID)
	}
	if strings.TrimSpace(manifest.Version) == "" || len(manifest.Version) > 32 {
		return fmt.Errorf("register plugin %q: version must be 1 to 32 characters", manifest.ID)
	}
	if manifest.AdminPath != "" && !strings.HasPrefix(manifest.AdminPath, "/admin/") {
		return fmt.Errorf("register plugin %q: admin_path must start with /admin/", manifest.ID)
	}
	seenDependencies := make(map[string]bool, len(manifest.Dependencies))
	for _, dependency := range manifest.Dependencies {
		dependency = strings.TrimSpace(dependency)
		if !idPattern.MatchString(dependency) {
			return fmt.Errorf("register plugin %q: dependency %q must be a plugin id", manifest.ID, dependency)
		}
		if dependency == manifest.ID {
			return fmt.Errorf("register plugin %q: a plugin cannot depend on itself", manifest.ID)
		}
		if seenDependencies[dependency] {
			return fmt.Errorf("register plugin %q: dependency %q is repeated", manifest.ID, dependency)
		}
		seenDependencies[dependency] = true
	}
	if manifest.ProductType != nil {
		if err := validateProductType(*manifest.ProductType); err != nil {
			return fmt.Errorf("register plugin %q: %w", manifest.ID, err)
		}
	}
	if err := validateCategories(manifest.Categories); err != nil {
		return fmt.Errorf("register plugin %q: %w", manifest.ID, err)
	}
	if err := validateStorefrontPages(manifest.StorefrontPages); err != nil {
		return fmt.Errorf("register plugin %q: %w", manifest.ID, err)
	}
	for _, existing := range r.order {
		for _, category := range existing.Manifest().Categories {
			for _, candidate := range manifest.Categories {
				if candidate.Slug == category.Slug {
					return fmt.Errorf("register plugin %q: category %q is already owned by %q",
						manifest.ID, candidate.Slug, existing.Manifest().ID)
				}
			}
		}
		for _, page := range existing.Manifest().StorefrontPages {
			for _, candidate := range manifest.StorefrontPages {
				if candidate.Path == page.Path {
					return fmt.Errorf("register plugin %q: storefront page %q is already owned by %q",
						manifest.ID, candidate.Path, existing.Manifest().ID)
				}
			}
		}
	}
	if err := validateSettings(manifest.Settings); err != nil {
		return fmt.Errorf("register plugin %q: %w", manifest.ID, err)
	}

	seenRoutes := make(map[string]bool)
	for _, route := range plugin.Routes() {
		method := strings.ToUpper(strings.TrimSpace(route.Method))
		if !validMethod(method) {
			return fmt.Errorf("register plugin %q: route %s %q has an unsupported method", manifest.ID, route.Method, route.Pattern)
		}
		pattern := strings.TrimSpace(route.Pattern)
		if !strings.HasPrefix(pattern, "/") || strings.Contains(pattern, "..") ||
			strings.Contains(pattern, "//") || strings.ContainsAny(pattern, " \t\r\n") {
			return fmt.Errorf("register plugin %q: route %q must be a clean absolute path", manifest.ID, route.Pattern)
		}
		if strings.HasPrefix(pattern, "/admin") {
			return fmt.Errorf("register plugin %q: route %q must not include the /admin prefix; use Admin", manifest.ID, pattern)
		}
		if route.Handler == nil {
			return fmt.Errorf("register plugin %q: route %s %q has no handler", manifest.ID, method, pattern)
		}
		key := method + " " + pattern
		if seenRoutes[key] {
			return fmt.Errorf("register plugin %q: duplicate route %s", manifest.ID, key)
		}
		seenRoutes[key] = true
		if route.Webhook {
			if route.Admin {
				return fmt.Errorf("register plugin %q: webhook route %s %q cannot be an admin route", manifest.ID, method, pattern)
			}
			if method != "GET" && method != "POST" {
				return fmt.Errorf("register plugin %q: webhook route %s %q must use GET or POST", manifest.ID, method, pattern)
			}
			if strings.ContainsAny(pattern, "{}*") {
				return fmt.Errorf("register plugin %q: webhook route %q must be static", manifest.ID, pattern)
			}
		}
	}

	r.order = append(r.order, plugin)
	r.byID[manifest.ID] = plugin
	r.enabled[manifest.ID] = true
	return nil
}

func (r *Registry) All() []Plugin {
	out := make([]Plugin, len(r.order))
	copy(out, r.order)
	return out
}

func (r *Registry) Get(id string) (Plugin, bool) {
	plugin, ok := r.byID[id]
	return plugin, ok
}

// IsWebhook reports whether method and path address a registered route that a
// third party calls. The path is the full request path, including /api/plugins.
func (r *Registry) IsWebhook(method, path string) bool {
	method = strings.ToUpper(strings.TrimSpace(method))
	r.mu.RLock()
	defer r.mu.RUnlock()
	for _, plugin := range r.order {
		id := plugin.Manifest().ID
		for _, route := range plugin.Routes() {
			if !route.Webhook {
				continue
			}
			if method == strings.ToUpper(route.Method) &&
				path == "/api/plugins/"+id+route.Pattern {
				return true
			}
		}
	}
	return false
}

// ValidateDependencies rejects unknown dependencies and cycles after every
// plugin is registered.
func (r *Registry) ValidateDependencies() error {
	r.mu.RLock()
	defer r.mu.RUnlock()

	for id, plugin := range r.byID {
		for _, dependency := range plugin.Manifest().Dependencies {
			dependency = store.NormalizePluginID(dependency)
			if _, ok := r.byID[dependency]; !ok {
				return fmt.Errorf("plugin %q depends on unknown plugin %q", id, dependency)
			}
		}
	}

	const (
		unvisited = iota
		visiting
		visited
	)
	state := make(map[string]int, len(r.byID))
	var visit func(string) error
	visit = func(id string) error {
		switch state[id] {
		case visiting:
			return fmt.Errorf("plugin dependency cycle includes %q", id)
		case visited:
			return nil
		}
		state[id] = visiting
		for _, dependency := range r.byID[id].Manifest().Dependencies {
			if err := visit(store.NormalizePluginID(dependency)); err != nil {
				return err
			}
		}
		state[id] = visited
		return nil
	}
	for id := range r.byID {
		if err := visit(id); err != nil {
			return err
		}
	}
	return nil
}

// SetStates applies persisted enable/disable flags. Plugins without a stored
// row stay enabled.
func (r *Registry) SetStates(states map[string]bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	for id := range r.byID {
		r.enabled[id] = true
		if value, ok := states[id]; ok {
			r.enabled[id] = value
		}
	}
}

// SetEnabled changes the desired state of one registered plugin.
func (r *Registry) SetEnabled(id string, enabled bool) bool {
	id = store.NormalizePluginID(id)
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.byID[id]; !ok {
		return false
	}
	r.enabled[id] = enabled
	return true
}

// Enabled is the admin's desired state. Core products are always enabled.
func (r *Registry) Enabled(id string) bool {
	id = store.NormalizePluginID(id)
	r.mu.RLock()
	defer r.mu.RUnlock()
	return r.enabledLocked(id)
}

func (r *Registry) enabledLocked(id string) bool {
	if id == store.CorePluginID {
		return true
	}
	if _, ok := r.byID[id]; !ok {
		return true
	}
	if value, ok := r.enabled[id]; ok {
		return value
	}
	return true
}

// EffectiveEnabled also requires every dependency to be enabled.
func (r *Registry) EffectiveEnabled(id string) bool {
	id = store.NormalizePluginID(id)
	r.mu.RLock()
	defer r.mu.RUnlock()
	return r.effectiveEnabledLocked(id, make(map[string]bool))
}

func (r *Registry) effectiveEnabledLocked(id string, seen map[string]bool) bool {
	if id == store.CorePluginID {
		return true
	}
	plugin, ok := r.byID[id]
	if !ok {
		return true
	}
	if seen[id] {
		return true
	}
	seen[id] = true
	if !r.enabledLocked(id) {
		return false
	}
	for _, dependency := range plugin.Manifest().Dependencies {
		if !r.effectiveEnabledLocked(store.NormalizePluginID(dependency), seen) {
			return false
		}
	}
	return true
}

// Dependents returns direct and transitive dependents, sorted by id.
func (r *Registry) Dependents(id string) []string {
	id = store.NormalizePluginID(id)
	r.mu.RLock()
	defer r.mu.RUnlock()
	return r.dependentsLocked(id, make(map[string]bool))
}

func (r *Registry) dependentsLocked(id string, seen map[string]bool) []string {
	out := make([]string, 0)
	for pluginID, plugin := range r.byID {
		if pluginID == id {
			continue
		}
		for _, dependency := range plugin.Manifest().Dependencies {
			if store.NormalizePluginID(dependency) != id {
				continue
			}
			if !seen[pluginID] {
				seen[pluginID] = true
				out = append(out, pluginID)
			}
			out = append(out, r.dependentsLocked(pluginID, seen)...)
			break
		}
	}
	if len(out) > 1 {
		sort.Strings(out)
	}
	return out
}

// EnabledDependents is the warning set for disabling a plugin.
func (r *Registry) EnabledDependents(id string) []string {
	r.mu.RLock()
	defer r.mu.RUnlock()
	dependents := r.dependentsLocked(store.NormalizePluginID(id), make(map[string]bool))
	out := make([]string, 0, len(dependents))
	for _, dependent := range dependents {
		if r.enabledLocked(dependent) {
			out = append(out, dependent)
		}
	}
	return out
}

// BlockedBy lists disabled dependencies that make an enabled plugin
// unavailable.
func (r *Registry) BlockedBy(id string) []string {
	id = store.NormalizePluginID(id)
	r.mu.RLock()
	defer r.mu.RUnlock()
	return r.blockedByLocked(id, make(map[string]bool))
}

func (r *Registry) blockedByLocked(id string, seen map[string]bool) []string {
	plugin, ok := r.byID[id]
	if !ok || seen[id] {
		return nil
	}
	seen[id] = true
	out := make([]string, 0)
	for _, dependency := range plugin.Manifest().Dependencies {
		dependency = store.NormalizePluginID(dependency)
		if !r.enabledLocked(dependency) {
			out = append(out, dependency)
			continue
		}
		out = append(out, r.blockedByLocked(dependency, seen)...)
	}
	if len(out) > 1 {
		sort.Strings(out)
	}
	return out
}

func (r *Registry) Manifests() []Manifest {
	out := make([]Manifest, 0, len(r.order))
	for _, plugin := range r.order {
		out = append(out, plugin.Manifest())
	}
	return out
}

type RegisteredMigration struct {
	PluginID  string
	Migration Migration
}

func (r *Registry) Migrations() []RegisteredMigration {
	out := make([]RegisteredMigration, 0)
	for _, plugin := range r.order {
		migrator, ok := plugin.(Migrator)
		if !ok {
			continue
		}
		for _, migration := range migrator.Migrations() {
			out = append(out, RegisteredMigration{
				PluginID:  plugin.Manifest().ID,
				Migration: migration,
			})
		}
	}
	return out
}

func (r *Registry) ProductType(id string) *ProductType {
	plugin, ok := r.byID[id]
	if !ok {
		return nil
	}
	return plugin.Manifest().ProductType
}

func (r *Registry) CheckoutHook(id string) CheckoutHook {
	plugin, ok := r.byID[id]
	if !ok {
		return nil
	}
	hook, _ := plugin.(CheckoutHook)
	return hook
}

func (r *Registry) OrderStatusHooks() []OrderStatusHook {
	out := make([]OrderStatusHook, 0)
	for _, plugin := range r.order {
		if hook, ok := plugin.(OrderStatusHook); ok {
			out = append(out, hook)
		}
	}
	return out
}

func (r *Registry) EnrichProduct(ctx context.Context, product store.Product) map[string]any {
	plugin, ok := r.byID[product.PluginID]
	if !ok {
		return nil
	}
	enricher, ok := plugin.(ProductEnricher)
	if !ok {
		return nil
	}
	data, err := enricher.EnrichProduct(ctx, product)
	if err != nil {
		return nil
	}
	return data
}

func validateProductType(productType ProductType) error {
	if strings.TrimSpace(productType.Label) == "" || len([]rune(productType.Label)) > 60 {
		return fmt.Errorf("product type label must be 1 to 60 characters")
	}
	seen := make(map[string]bool)
	for _, field := range productType.Fields {
		if !keyPattern.MatchString(field.Key) {
			return fmt.Errorf("option field %q must be lowercase letters, numbers and underscores", field.Key)
		}
		if seen[field.Key] {
			return fmt.Errorf("option field %q is repeated", field.Key)
		}
		seen[field.Key] = true
		if strings.TrimSpace(field.Label) == "" || len([]rune(field.Label)) > 60 {
			return fmt.Errorf("option field %q needs a label of 1 to 60 characters", field.Key)
		}
		if field.Type != "select" && field.Type != "text" && field.Type != "textarea" && field.Type != "list" {
			return fmt.Errorf("option field %q has an unsupported type %q", field.Key, field.Type)
		}
		if field.Type == "select" && len(field.Suggestions) > 12 {
			return fmt.Errorf("option field %q has more than 12 suggestions", field.Key)
		}
	}
	return nil
}

func validateCategories(categories []Category) error {
	seen := make(map[string]bool, len(categories))
	for _, category := range categories {
		if !idPattern.MatchString(category.Slug) {
			return fmt.Errorf("category %q must be lowercase letters, numbers, dashes or underscores", category.Slug)
		}
		if seen[category.Slug] {
			return fmt.Errorf("category %q is repeated", category.Slug)
		}
		seen[category.Slug] = true
		if strings.TrimSpace(category.Name) == "" || len([]rune(category.Name)) > 80 {
			return fmt.Errorf("category %q needs a name of 1 to 80 characters", category.Slug)
		}
		if len([]rune(category.Description)) > 300 {
			return fmt.Errorf("category %q has a description over 300 characters", category.Slug)
		}
	}
	return nil
}

func validateStorefrontPages(pages []StorefrontPage) error {
	reserved := map[string]bool{
		"/": true, "/shop": true, "/about": true, "/contact": true, "/cart": true,
		"/checkout": true, "/account": true, "/admin": true, "/order": true, "/product": true,
	}
	seen := make(map[string]bool, len(pages))
	for _, page := range pages {
		if !strings.HasPrefix(page.Path, "/") || strings.HasPrefix(page.Path, "/admin") ||
			strings.Contains(page.Path, "..") || strings.Contains(page.Path, "//") ||
			strings.ContainsAny(page.Path, " \t\r\n\\%?#") || len(page.Path) > 200 {
			return fmt.Errorf("storefront page path %q must be a clean public path", page.Path)
		}
		if reserved[page.Path] {
			return fmt.Errorf("storefront page path %q is reserved by the core", page.Path)
		}
		if seen[page.Path] {
			return fmt.Errorf("storefront page path %q is repeated", page.Path)
		}
		seen[page.Path] = true
		if strings.TrimSpace(page.Label) == "" || len([]rune(page.Label)) > 60 {
			return fmt.Errorf("storefront page %q needs a label of 1 to 60 characters", page.Path)
		}
	}
	return nil
}

func validateSettings(settings []SettingField) error {
	seen := make(map[string]bool)
	for _, field := range settings {
		if !keyPattern.MatchString(field.Key) {
			return fmt.Errorf("setting %q must be lowercase letters, numbers and underscores", field.Key)
		}
		if seen[field.Key] {
			return fmt.Errorf("setting %q is repeated", field.Key)
		}
		seen[field.Key] = true
		if strings.TrimSpace(field.Label) == "" || len([]rune(field.Label)) > 80 {
			return fmt.Errorf("setting %q needs a label of 1 to 80 characters", field.Key)
		}
		switch field.Type {
		case "text", "secret", "number", "boolean":
		case "select":
			if len(field.Options) == 0 {
				return fmt.Errorf("setting %q needs at least one option", field.Key)
			}
		default:
			return fmt.Errorf("setting %q has an unsupported type %q", field.Key, field.Type)
		}
		if field.MaxLength < 0 || field.MaxLength > 4096 {
			return fmt.Errorf("setting %q has an invalid max length", field.Key)
		}
	}
	return nil
}

func validMethod(method string) bool {
	switch method {
	case "GET", "POST", "PUT", "PATCH", "DELETE":
		return true
	}
	return false
}
