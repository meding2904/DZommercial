package codemode

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"regexp"
	"slices"
	"sort"
	"strings"
	"unicode/utf8"

	"azahar/internal/ids"
	"azahar/internal/plugins"
	"azahar/internal/store"
	"azahar/internal/storefront"
)

const (
	maxActions      = 10
	maxProductPrice = 99999999
	maxProductStock = 100000
)

var orderStatuses = []string{"pending", "confirmed", "packed", "shipped", "delivered", "cancelled"}

var orderTransitions = map[string][]string{
	"pending":   {"confirmed", "cancelled"},
	"confirmed": {"packed", "cancelled"},
	"packed":    {"shipped", "cancelled"},
	"shipped":   {"delivered"},
	"delivered": {},
	"cancelled": {},
}

func knownOrderStatus(status string) bool {
	return slices.Contains(orderStatuses, status)
}

func canOrderTransition(from, to string) bool {
	return slices.Contains(orderTransitions[from], to)
}

var (
	slugPattern     = regexp.MustCompile(`^[a-z0-9]+(?:-[a-z0-9]+)*$`)
	pluginIDPattern = regexp.MustCompile(`^[a-z][a-z0-9_-]{1,31}$`)
	imageIDPattern  = regexp.MustCompile(`^img_[a-f0-9]{24}$`)

	errAlreadyApplied = errors.New("codemode plan was already applied")

	themePatchKeys = map[string]bool{
		"brand_name":       true,
		"tagline":          true,
		"description":      true,
		"hero_heading":     true,
		"hero_body":        true,
		"hero_alt":         true,
		"hero_cta":         true,
		"hero_image":       true,
		"statement":        true,
		"shelf_heading":    true,
		"shop_title":       true,
		"house_label":      true,
		"about_heading":    true,
		"about_body":       true,
		"about_image":      true,
		"about_alt":        true,
		"closing_heading":  true,
		"footer_note":      true,
		"footer_credit":    true,
		"contact_email":    true,
		"announcement":     true,
		"concerns_enabled": true,
		"font_display":     true,
		"font_body":        true,
		"font_scale":       true,
		"radius":           true,
		"light":            true,
		"dark":             true,
	}

	palettePatchKeys = map[string]bool{
		"ground":     true,
		"surface":    true,
		"ink":        true,
		"muted":      true,
		"accent":     true,
		"accent_ink": true,
	}
)

type Action struct {
	Type      string          `json:"type"`
	Patch     json.RawMessage `json:"patch,omitempty"`
	ProductID string          `json:"product_id,omitempty"`
	Product   *ProductDraft   `json:"product,omitempty"`
	OrderID   string          `json:"order_id,omitempty"`
	Status    string          `json:"status,omitempty"`
}

type ProductDraft struct {
	Name         string              `json:"name"`
	Slug         string              `json:"slug"`
	PriceCents   int64               `json:"price_cents"`
	CategorySlug string              `json:"category_slug"`
	PluginID     string              `json:"plugin_id"`
	Stock        int                 `json:"stock"`
	Image        string              `json:"image"`
	Description  string              `json:"description"`
	Attributes   map[string][]string `json:"attributes,omitempty"`
}

type Plan struct {
	Summary string   `json:"summary"`
	Actions []Action `json:"actions"`
}

type planHistoryDTO struct {
	ID          string `json:"id"`
	Instruction string `json:"instruction"`
	Summary     string `json:"summary"`
	Status      string `json:"status"`
	CreatedAt   string `json:"created_at"`
}

type actionResult struct {
	Type    string `json:"type"`
	OK      bool   `json:"ok"`
	Message string `json:"message"`
}

type productPatch struct {
	Name        *string `json:"name"`
	Subtitle    *string `json:"subtitle"`
	Description *string `json:"description"`
	PriceCents  *int64  `json:"price_cents"`
	Stock       *int    `json:"stock"`
	Active      *bool   `json:"active"`
}

type invalidPlanError struct {
	message string
}

func (e *invalidPlanError) Error() string { return e.message }

func invalidPlan(format string, args ...any) error {
	return &invalidPlanError{message: fmt.Sprintf(format, args...)}
}

func newPlanID() string { return ids.New("cmp") }

func planDTO(id string, plan Plan, status, createdAt string) map[string]any {
	return map[string]any{
		"id":         id,
		"summary":    plan.Summary,
		"actions":    plan.Actions,
		"status":     status,
		"created_at": createdAt,
	}
}

func encodePlan(plan Plan) (string, error) {
	raw, err := json.Marshal(plan)
	if err != nil {
		return "", err
	}
	return string(raw), nil
}

func parsePlan(raw string) (Plan, error) {
	text := strings.TrimSpace(raw)
	if strings.HasPrefix(text, "```") {
		if newline := strings.IndexByte(text, '\n'); newline >= 0 {
			text = text[newline+1:]
		}
		text = strings.TrimSpace(strings.TrimSuffix(strings.TrimSpace(text), "```"))
	}
	if text == "" {
		return Plan{}, errors.New("empty plan")
	}

	var envelope struct {
		Summary string            `json:"summary"`
		Actions []json.RawMessage `json:"actions"`
	}
	decoder := json.NewDecoder(strings.NewReader(text))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&envelope); err != nil {
		return Plan{}, fmt.Errorf("read plan: %w", err)
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return Plan{}, errors.New("plan must be one JSON object")
	}

	plan := Plan{
		Summary: strings.TrimSpace(envelope.Summary),
		Actions: make([]Action, 0, len(envelope.Actions)),
	}
	for index, rawAction := range envelope.Actions {
		action, err := parseAction(rawAction)
		if err != nil {
			return Plan{}, fmt.Errorf("action %d: %w", index+1, err)
		}
		plan.Actions = append(plan.Actions, action)
	}
	return plan, nil
}

func parseAction(raw json.RawMessage) (Action, error) {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil || fields == nil {
		return Action{}, errors.New("action must be a JSON object")
	}

	var actionType string
	if err := json.Unmarshal(fields["type"], &actionType); err != nil || strings.TrimSpace(actionType) == "" {
		return Action{}, errors.New("action needs a type")
	}
	actionType = strings.TrimSpace(actionType)

	var allowed map[string]bool
	switch actionType {
	case "theme.update":
		allowed = map[string]bool{"type": true, "patch": true}
	case "product.update":
		allowed = map[string]bool{"type": true, "product_id": true, "patch": true}
	case "product.archive":
		allowed = map[string]bool{"type": true, "product_id": true}
	case "product.create":
		allowed = map[string]bool{"type": true, "product": true}
	case "order.status":
		allowed = map[string]bool{"type": true, "order_id": true, "status": true}
	default:
		return Action{}, fmt.Errorf("unknown action type %q", actionType)
	}
	for key := range fields {
		if !allowed[key] {
			return Action{}, fmt.Errorf("%s does not take %q", actionType, key)
		}
	}

	if actionType == "theme.update" || actionType == "product.update" {
		if _, err := rawObject(fields["patch"]); err != nil {
			return Action{}, fmt.Errorf("%s needs a patch object", actionType)
		}
	}
	if actionType == "product.create" {
		if _, err := rawObject(fields["product"]); err != nil {
			return Action{}, errors.New("product.create needs a product object")
		}
	}
	if actionType == "order.status" {
		var orderID string
		if err := json.Unmarshal(fields["order_id"], &orderID); err != nil || strings.TrimSpace(orderID) == "" || len(orderID) > 64 {
			return Action{}, errors.New("order.status needs an order id")
		}
		var status string
		if err := json.Unmarshal(fields["status"], &status); err != nil || !knownOrderStatus(strings.TrimSpace(status)) {
			return Action{}, fmt.Errorf("order.status needs one of %s", strings.Join(orderStatuses, ", "))
		}
	}

	var action Action
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&action); err != nil {
		return Action{}, fmt.Errorf("read %s: %w", actionType, err)
	}
	action.Type = actionType
	return action, nil
}

func rawObject(raw json.RawMessage) (map[string]json.RawMessage, error) {
	if len(raw) == 0 || string(raw) == "null" {
		return nil, errors.New("expected a JSON object")
	}
	var object map[string]json.RawMessage
	if err := json.Unmarshal(raw, &object); err != nil || object == nil {
		return nil, errors.New("expected a JSON object")
	}
	return object, nil
}

func isNull(raw json.RawMessage) bool {
	return strings.TrimSpace(string(raw)) == "null"
}

func (p *Plugin) validatePlanShape(plan Plan, theme storefront.Theme) (storefront.Theme, error) {
	summary := strings.TrimSpace(plan.Summary)
	if length := utf8.RuneCountInString(summary); length < 1 || length > 300 {
		return theme, invalidPlan("the plan summary must be 1 to 300 characters")
	}
	if !plainText(summary) {
		return theme, invalidPlan("the plan summary must be plain text")
	}
	if len(plan.Actions) == 0 {
		return theme, invalidPlan("the instruction cannot be planned with the available actions")
	}
	if len(plan.Actions) > maxActions {
		return theme, invalidPlan("a plan takes at most %d actions", maxActions)
	}

	for index, action := range plan.Actions {
		prefix := fmt.Sprintf("action %d", index+1)
		switch action.Type {
		case "theme.update":
			if action.ProductID != "" || action.Product != nil {
				return theme, invalidPlan("%s: theme.update takes only a patch", prefix)
			}
			next, err := applyThemePatch(theme, action.Patch)
			if err != nil {
				return theme, invalidPlan("%s: %s", prefix, err.Error())
			}
			theme = next
		case "product.update":
			if strings.TrimSpace(action.ProductID) == "" {
				return theme, invalidPlan("%s: choose a product id", prefix)
			}
			if _, err := parseProductPatch(action.Patch); err != nil {
				return theme, invalidPlan("%s: %s", prefix, err.Error())
			}
		case "product.archive":
			if strings.TrimSpace(action.ProductID) == "" {
				return theme, invalidPlan("%s: choose a product id", prefix)
			}
			if len(action.Patch) > 0 || action.Product != nil {
				return theme, invalidPlan("%s: product.archive takes only a product id", prefix)
			}
		case "product.create":
			if action.ProductID != "" || len(action.Patch) > 0 {
				return theme, invalidPlan("%s: product.create takes only a product", prefix)
			}
			if err := p.validateProductDraft(action.Product); err != nil {
				return theme, invalidPlan("%s: %s", prefix, err.Error())
			}
		case "order.status":
			if action.ProductID != "" || action.Product != nil || len(action.Patch) > 0 {
				return theme, invalidPlan("%s: order.status takes only an order id and status", prefix)
			}
			if strings.TrimSpace(action.OrderID) == "" {
				return theme, invalidPlan("%s: choose an order id", prefix)
			}
			if !knownOrderStatus(action.Status) {
				return theme, invalidPlan("%s: choose a known order status", prefix)
			}
		default:
			return theme, invalidPlan("%s: unknown action type %q", prefix, action.Type)
		}
	}
	return theme, nil
}

func applyThemePatch(theme storefront.Theme, raw json.RawMessage) (storefront.Theme, error) {
	patch, err := rawObject(raw)
	if err != nil {
		return theme, errors.New("theme.update needs a patch object")
	}
	if len(patch) == 0 {
		return theme, errors.New("theme.update needs at least one field")
	}
	for key, value := range patch {
		if !themePatchKeys[key] {
			return theme, fmt.Errorf("%q is not a theme field", key)
		}
		if key == "light" || key == "dark" {
			palette, err := rawObject(value)
			if err != nil {
				return theme, fmt.Errorf("%q must be an object", key)
			}
			if len(palette) == 0 {
				return theme, fmt.Errorf("%q needs at least one colour", key)
			}
			for colour, colourValue := range palette {
				if !palettePatchKeys[colour] {
					return theme, fmt.Errorf("%q is not a colour in %q", colour, key)
				}
				if isNull(colourValue) {
					return theme, fmt.Errorf("%s.%s cannot be empty", key, colour)
				}
			}
			continue
		}
		if isNull(value) {
			return theme, fmt.Errorf("%q cannot be empty", key)
		}
	}

	current, err := themeObject(theme)
	if err != nil {
		return theme, err
	}
	for key, value := range patch {
		if key == "light" || key == "dark" {
			existing, _ := rawObject(current[key])
			if existing == nil {
				existing = map[string]json.RawMessage{}
			}
			palette, _ := rawObject(value)
			for colour, colourValue := range palette {
				existing[colour] = colourValue
			}
			encoded, err := json.Marshal(existing)
			if err != nil {
				return theme, err
			}
			current[key] = encoded
			continue
		}
		current[key] = value
	}

	encoded, err := json.Marshal(current)
	if err != nil {
		return theme, err
	}
	next, err := storefront.Parse(string(encoded))
	if err != nil {
		return theme, errors.New("the theme patch has a value of the wrong type")
	}
	next.UpdatedAt = ""
	if fields := storefront.Validate(next); len(fields) > 0 {
		return theme, errors.New(firstFieldMessage(fields))
	}
	return next, nil
}

func themeObject(theme storefront.Theme) (map[string]json.RawMessage, error) {
	raw, err := json.Marshal(theme)
	if err != nil {
		return nil, err
	}
	object, err := rawObject(raw)
	if err != nil {
		return nil, err
	}
	delete(object, "updated_at")
	return object, nil
}

func parseProductPatch(raw json.RawMessage) (productPatch, error) {
	fields, err := rawObject(raw)
	if err != nil {
		return productPatch{}, errors.New("product.update needs a patch object")
	}
	if len(fields) == 0 {
		return productPatch{}, errors.New("product.update needs at least one field")
	}
	allowed := map[string]bool{
		"name":        true,
		"subtitle":    true,
		"description": true,
		"price_cents": true,
		"stock":       true,
		"active":      true,
	}
	for key := range fields {
		if !allowed[key] {
			return productPatch{}, fmt.Errorf("%q is not an editable product field", key)
		}
		if isNull(fields[key]) {
			return productPatch{}, fmt.Errorf("%q cannot be empty", key)
		}
	}

	var patch productPatch
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&patch); err != nil {
		return productPatch{}, fmt.Errorf("product patch has a value of the wrong type")
	}

	if patch.Name != nil {
		name := strings.TrimSpace(*patch.Name)
		if utf8.RuneCountInString(name) < 2 || utf8.RuneCountInString(name) > 120 {
			return productPatch{}, errors.New("the product name must be 2 to 120 characters")
		}
		if !plainText(name) {
			return productPatch{}, errors.New("the product name must be plain text")
		}
		patch.Name = &name
	}
	if patch.Subtitle != nil {
		subtitle := strings.TrimSpace(*patch.Subtitle)
		if utf8.RuneCountInString(subtitle) > 160 {
			return productPatch{}, errors.New("the subtitle must stay under 161 characters")
		}
		patch.Subtitle = &subtitle
	}
	if patch.Description != nil {
		description := strings.TrimSpace(*patch.Description)
		if utf8.RuneCountInString(description) > 4000 {
			return productPatch{}, errors.New("the description must stay under 4001 characters")
		}
		patch.Description = &description
	}
	if patch.PriceCents != nil && (*patch.PriceCents < 1 || *patch.PriceCents > maxProductPrice) {
		return productPatch{}, fmt.Errorf("the price must be between 1 and %d cents", maxProductPrice)
	}
	if patch.Stock != nil && (*patch.Stock < 0 || *patch.Stock > maxProductStock) {
		return productPatch{}, fmt.Errorf("the stock must be between 0 and %d", maxProductStock)
	}
	return patch, nil
}

func (p *Plugin) validateProductDraft(product *ProductDraft) error {
	if product == nil {
		return errors.New("choose the product to create")
	}

	product.Name = strings.TrimSpace(product.Name)
	if length := utf8.RuneCountInString(product.Name); length < 2 || length > 120 {
		return errors.New("the product name must be 2 to 120 characters")
	}
	if !plainText(product.Name) {
		return errors.New("the product name must be plain text")
	}
	product.Slug = strings.TrimSpace(product.Slug)
	if len(product.Slug) < 1 || len(product.Slug) > 120 || !slugPattern.MatchString(product.Slug) {
		return errors.New("the product slug must use lowercase letters, numbers and hyphens")
	}
	product.CategorySlug = strings.TrimSpace(product.CategorySlug)
	if product.CategorySlug == "" || !slugPattern.MatchString(product.CategorySlug) {
		return errors.New("choose a valid category slug")
	}
	product.PluginID = strings.TrimSpace(product.PluginID)
	if !pluginIDPattern.MatchString(product.PluginID) {
		return errors.New("choose a valid plugin id")
	}
	if product.PriceCents < 1 || product.PriceCents > maxProductPrice {
		return fmt.Errorf("the price must be between 1 and %d cents", maxProductPrice)
	}
	if product.Stock < 0 || product.Stock > maxProductStock {
		return fmt.Errorf("the stock must be between 0 and %d", maxProductStock)
	}
	if !validImagePath(product.Image) {
		return errors.New("use an uploaded /api/images/ or /images/ path for the image")
	}
	product.Description = strings.TrimSpace(product.Description)
	if utf8.RuneCountInString(product.Description) > 4000 {
		return errors.New("the description must stay under 4001 characters")
	}

	attributes, err := normalizeAttributes(product.Attributes)
	if err != nil {
		return err
	}
	product.Attributes = attributes

	pluginID := store.NormalizePluginID(product.PluginID)
	product.PluginID = pluginID
	if pluginID == store.CorePluginID {
		if len(attributes) > 0 {
			return errors.New("a core product does not take plugin options")
		}
		return nil
	}
	if p.registry == nil {
		return errors.New("the installed plugin list is unavailable")
	}
	plugin, ok := p.registry.Get(pluginID)
	if !ok {
		return errors.New("choose an installed plugin")
	}
	productType := plugin.Manifest().ProductType
	if productType == nil {
		return errors.New("that plugin does not define a product type")
	}
	return validateProductAttributes(productType, attributes)
}

func normalizeAttributes(attributes map[string][]string) (map[string][]string, error) {
	normalized := make(map[string][]string, len(attributes))
	for key, values := range attributes {
		key = strings.TrimSpace(key)
		if key == "" {
			return nil, errors.New("option names cannot be empty")
		}
		if _, exists := normalized[key]; exists {
			return nil, errors.New("option names must be unique")
		}
		trimmed := make([]string, 0, len(values))
		for _, value := range values {
			trimmed = append(trimmed, strings.TrimSpace(value))
		}
		normalized[key] = trimmed
	}
	return normalized, nil
}

func validateProductAttributes(productType *plugins.ProductType, attributes map[string][]string) error {
	known := make(map[string]plugins.OptionField, len(productType.Fields))
	for _, field := range productType.Fields {
		known[field.Key] = field
		values := attributes[field.Key]
		if field.Required && len(values) == 0 {
			return fmt.Errorf("add at least one %s value", strings.ToLower(field.Label))
		}
		switch field.Type {
		case "text", "textarea":
			maximum := 120
			if field.Type == "textarea" {
				maximum = 2000
			}
			if len(values) > 1 {
				return fmt.Errorf("%s takes one value", strings.ToLower(field.Label))
			}
			if len(values) == 1 {
				value := values[0]
				if value == "" || utf8.RuneCountInString(value) > maximum || !plainText(value) {
					return fmt.Errorf("%s must be plain text under %d characters", strings.ToLower(field.Label), maximum+1)
				}
			}
		case "select", "list":
			if len(values) > 12 {
				return fmt.Errorf("%s takes at most 12 values", strings.ToLower(field.Label))
			}
			seen := make(map[string]bool, len(values))
			for _, value := range values {
				if value == "" || utf8.RuneCountInString(value) > 80 || !plainText(value) {
					return fmt.Errorf("%s values must be plain text under 81 characters", strings.ToLower(field.Label))
				}
				if seen[value] {
					return fmt.Errorf("%s values must be unique", strings.ToLower(field.Label))
				}
				seen[value] = true
			}
		}
	}
	for key := range attributes {
		if _, ok := known[key]; !ok {
			return errors.New("remove options this product type does not define")
		}
	}
	return nil
}

func (p *Plugin) validatePlanReferences(ctx context.Context, st *store.Store, plan Plan) error {
	if st == nil {
		return errors.New("the store is unavailable")
	}
	for index, action := range plan.Actions {
		prefix := fmt.Sprintf("action %d", index+1)
		switch action.Type {
		case "product.update", "product.archive":
			if _, err := st.ProductByID(ctx, action.ProductID, false); err != nil {
				if errors.Is(err, sql.ErrNoRows) {
					return invalidPlan("%s: product %q does not exist", prefix, action.ProductID)
				}
				return err
			}
		case "product.create":
			product := action.Product
			if product == nil {
				return invalidPlan("%s: choose the product to create", prefix)
			}
			category, err := st.CategoryBySlug(ctx, product.CategorySlug)
			if err != nil {
				if errors.Is(err, sql.ErrNoRows) {
					return invalidPlan("%s: category %q does not exist", prefix, product.CategorySlug)
				}
				return err
			}
			if category.PluginID != store.NormalizePluginID(product.PluginID) {
				return invalidPlan("%s: category %q belongs to another product type", prefix, product.CategorySlug)
			}
			if _, err := st.ProductBySlug(ctx, product.Slug, false); err == nil {
				return invalidPlan("%s: slug %q is already in use", prefix, product.Slug)
			} else if !errors.Is(err, sql.ErrNoRows) {
				return err
			}
		case "order.status":
			order, err := st.OrderByID(ctx, action.OrderID)
			if err != nil {
				if errors.Is(err, sql.ErrNoRows) {
					return invalidPlan("%s: order %q does not exist", prefix, action.OrderID)
				}
				return err
			}
			if !canOrderTransition(order.Status, action.Status) {
				return invalidPlan("%s: order %s cannot move from %s to %s",
					prefix, order.Number, order.Status, action.Status)
			}
		}
	}
	return nil
}

// planBaseline records the updated_at of every entity a preview can touch.
// The snapshot is taken from the exact data the model saw, not re-read after
// the provider answers, so a concurrent edit during planning is caught at
// apply. Absolute wall-clock comparison is avoided because imported or seeded
// rows may carry future timestamps.
type productBaselineFields struct {
	Name        string `json:"name"`
	Subtitle    string `json:"subtitle"`
	Description string `json:"description"`
	PriceCents  int64  `json:"price_cents"`
	Stock       int    `json:"stock"`
	Active      bool   `json:"active"`
}

// productFingerprint covers exactly the fields a codemode product action can
// write. Apply compares it inside the write transaction, so a stock change made
// by checkout or the phone shelf is caught even when the updated_at second is
// the same.
func productFingerprint(product store.Product) string {
	raw, _ := json.Marshal(productBaselineFields{
		Name:        product.Name,
		Subtitle:    product.Subtitle,
		Description: product.Description,
		PriceCents:  product.PriceCents,
		Stock:       product.Stock,
		Active:      product.Active,
	})
	return string(raw)
}

// planBaseline records the exact values the model saw. Apply compares these
// inside the write transaction, not against wall-clock timestamps.
func (p *Plugin) planBaseline(plan Plan, products []store.Product, orders []promptOrder, theme storefront.Theme) (string, error) {
	productSnapshots := make(map[string]string, len(products))
	for _, product := range products {
		productSnapshots[product.ID] = productFingerprint(product)
	}
	orderSnapshots := make(map[string]string, len(orders))
	for _, order := range orders {
		orderSnapshots[order.ID] = order.Status
	}
	themeJSON, err := storefront.Encode(theme)
	if err != nil {
		return "", err
	}

	baseline := make(map[string]string)
	for _, action := range plan.Actions {
		switch action.Type {
		case "product.update", "product.archive":
			value, ok := productSnapshots[action.ProductID]
			if !ok {
				return "", invalidPlan("product %q was not in this preview; preview the change again", action.ProductID)
			}
			baseline["product:"+action.ProductID] = value
		case "order.status":
			value, ok := orderSnapshots[action.OrderID]
			if !ok {
				return "", invalidPlan("order %q was not in this preview; preview the change again", action.OrderID)
			}
			baseline["order:"+action.OrderID] = value
		case "theme.update":
			baseline["theme"] = themeJSON
		}
	}
	raw, err := json.Marshal(baseline)
	if err != nil {
		return "", err
	}
	return string(raw), nil
}

func firstFieldMessage(fields map[string]string) string {
	keys := make([]string, 0, len(fields))
	for key := range fields {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	key := keys[0]
	return key + ": " + fields[key]
}

func plainText(value string) bool {
	for _, r := range value {
		if r == '\n' || r == '\t' {
			continue
		}
		if r < 0x20 || r == 0x7f {
			return false
		}
	}
	return true
}

func validImagePath(value string) bool {
	value = strings.TrimSpace(value)
	if value == "" || len(value) > 500 {
		return false
	}
	if strings.Contains(value, "..") || strings.ContainsAny(value, "\\%?#") {
		return false
	}
	if strings.HasPrefix(value, "/api/images/") {
		return imageIDPattern.MatchString(strings.TrimPrefix(value, "/api/images/"))
	}
	return strings.HasPrefix(value, "/images/")
}

func encodeResults(results []actionResult) (string, error) {
	if results == nil {
		results = []actionResult{}
	}
	raw, err := json.Marshal(map[string]any{"results": results})
	if err != nil {
		return "", err
	}
	return string(raw), nil
}
