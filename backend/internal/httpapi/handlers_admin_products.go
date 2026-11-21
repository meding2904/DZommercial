package httpapi

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"sort"
	"strings"
	"time"

	"azahar/internal/ids"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

type productCreatePayload struct {
	Name           string              `json:"name"`
	Slug           string              `json:"slug"`
	Subtitle       string              `json:"subtitle"`
	Description    string              `json:"description"`
	PriceCents     int64               `json:"price_cents"`
	CompareAtCents *int64              `json:"compare_at_cents"`
	SalePriceCents *int64              `json:"sale_price_cents"`
	SaleStartsAt   string              `json:"sale_starts_at"`
	SaleEndsAt     string              `json:"sale_ends_at"`
	Size           string              `json:"size"`
	CategorySlug   string              `json:"category_slug"`
	Concerns       []string            `json:"concerns"`
	Ingredients    []string            `json:"ingredients"`
	HowTo          string              `json:"how_to"`
	Image          string              `json:"image"`
	Gallery        []string            `json:"gallery"`
	Stock          int                 `json:"stock"`
	Featured       bool                `json:"featured"`
	Active         *bool               `json:"active"`
	PluginID       string              `json:"plugin_id"`
	Attributes     map[string][]string `json:"attributes"`
}

type productPatch struct {
	Name           *string              `json:"name"`
	Slug           *string              `json:"slug"`
	Subtitle       *string              `json:"subtitle"`
	Description    *string              `json:"description"`
	PriceCents     *int64               `json:"price_cents"`
	CompareAtCents *int64               `json:"compare_at_cents"`
	SalePriceCents *int64               `json:"sale_price_cents"`
	SaleStartsAt   *string              `json:"sale_starts_at"`
	SaleEndsAt     *string              `json:"sale_ends_at"`
	Size           *string              `json:"size"`
	CategorySlug   *string              `json:"category_slug"`
	Concerns       *[]string            `json:"concerns"`
	Ingredients    *[]string            `json:"ingredients"`
	HowTo          *string              `json:"how_to"`
	Image          *string              `json:"image"`
	Gallery        *[]string            `json:"gallery"`
	Stock          *int                 `json:"stock"`
	Featured       *bool                `json:"featured"`
	Active         *bool                `json:"active"`
	PluginID       *string              `json:"plugin_id"`
	Attributes     *map[string][]string `json:"attributes"`
}

func (s *Server) handleAdminProducts(w http.ResponseWriter, r *http.Request) {
	products, err := s.store.ListProducts(r.Context(), store.ProductFilter{
		IncludeInactive: true,
		Limit:           500,
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"products": s.toProducts(r.Context(), products, true)})
}

func (s *Server) handleCreateProduct(w http.ResponseWriter, r *http.Request) {
	payload, _, bad := decodeJSON[productCreatePayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	now := time.Now().UTC()
	salePrice, saleStarts, saleEnds, saleFields := parseSaleWindow(
		payload.SalePriceCents, payload.SaleStartsAt, payload.SaleEndsAt)
	product := store.Product{
		ID:             ids.New("prd"),
		Name:           strings.TrimSpace(payload.Name),
		Slug:           strings.TrimSpace(payload.Slug),
		Subtitle:       strings.TrimSpace(payload.Subtitle),
		Description:    strings.TrimSpace(payload.Description),
		PriceCents:     payload.PriceCents,
		CompareAtCents: payload.CompareAtCents,
		SalePriceCents: salePrice,
		SaleStartsAt:   saleStarts,
		SaleEndsAt:     saleEnds,
		Currency:       "DZD",
		Size:           strings.TrimSpace(payload.Size),
		Concerns:       trimAll(payload.Concerns),
		Ingredients:    trimAll(payload.Ingredients),
		HowTo:          strings.TrimSpace(payload.HowTo),
		Image:          strings.TrimSpace(payload.Image),
		Gallery:        trimAll(payload.Gallery),
		Stock:          payload.Stock,
		Featured:       payload.Featured,
		Active:         true,
		PluginID:       store.NormalizePluginID(strings.TrimSpace(payload.PluginID)),
		Attributes:     normalizeAttributes(payload.Attributes),
		CreatedAt:      now,
		UpdatedAt:      now,
	}
	if payload.Active != nil {
		product.Active = *payload.Active
	}

	fields := validateProduct(product)
	for key, message := range saleFields {
		fields[key] = message
	}
	for key, message := range s.validateProductAttributes(product.PluginID, product.Attributes) {
		fields[key] = message
	}
	category, err := s.store.CategoryBySlug(r.Context(), strings.TrimSpace(payload.CategorySlug))
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			fields["category_slug"] = "Choose a category."
		} else {
			writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
	} else {
		if category.PluginID != store.NormalizePluginID(product.PluginID) {
			fields["category_slug"] = "Choose a category that belongs to this product type."
		} else {
			product.Category = category
		}
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}

	taken, err := s.slugTaken(r.Context(), product.Slug, "")
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	if taken {
		writeProblem(w, problemFields(409, "conflict", "That slug is already in use.",
			map[string]string{"slug": "This slug is already in use."}))
		return
	}

	actor, _ := currentAuth(r)
	detail, _ := json.Marshal(map[string]string{"slug": product.Slug})
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.CreateProduct(r.Context(), tx, product); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "product.create",
			EntityType:  "product",
			EntityID:    product.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		if isUniqueViolation(err) {
			writeProblem(w, problemFields(409, "conflict", "That slug is already in use.",
				map[string]string{"slug": "This slug is already in use."}))
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	writeJSON(w, http.StatusCreated, map[string]any{"product": s.toProduct(r.Context(), product, true)})
}

func (s *Server) handleUpdateProduct(w http.ResponseWriter, r *http.Request) {
	id := chi.URLParam(r, "id")
	existing, err := s.store.ProductByID(r.Context(), id, false)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeError(w, http.StatusNotFound, "not_found", "Product not found.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	patch, raw, bad := decodeJSON[productPatch](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}

	p := existing
	fields := make(map[string]string)
	nullField := func(key string) { fields[key] = "This field cannot be empty." }

	if _, ok := raw["name"]; ok {
		if patch.Name == nil {
			nullField("name")
		} else {
			p.Name = strings.TrimSpace(*patch.Name)
		}
	}
	if _, ok := raw["slug"]; ok {
		if patch.Slug == nil {
			nullField("slug")
		} else {
			p.Slug = strings.TrimSpace(*patch.Slug)
		}
	}
	if _, ok := raw["subtitle"]; ok {
		if patch.Subtitle == nil {
			nullField("subtitle")
		} else {
			p.Subtitle = strings.TrimSpace(*patch.Subtitle)
		}
	}
	if _, ok := raw["description"]; ok {
		if patch.Description == nil {
			nullField("description")
		} else {
			p.Description = strings.TrimSpace(*patch.Description)
		}
	}
	if _, ok := raw["price_cents"]; ok {
		if patch.PriceCents == nil {
			nullField("price_cents")
		} else {
			p.PriceCents = *patch.PriceCents
		}
	}
	if _, ok := raw["compare_at_cents"]; ok {
		p.CompareAtCents = patch.CompareAtCents
	}
	if _, ok := raw["sale_price_cents"]; ok {
		if patch.SalePriceCents == nil || *patch.SalePriceCents == 0 {
			p.SalePriceCents = nil
		} else {
			value := *patch.SalePriceCents
			p.SalePriceCents = &value
			if !p.SaleWindow().PricePositive() {
				fields["sale_price_cents"] = "Enter a sale price above 0."
			}
		}
	}
	if _, ok := raw["sale_starts_at"]; ok {
		if patch.SaleStartsAt == nil || strings.TrimSpace(*patch.SaleStartsAt) == "" {
			p.SaleStartsAt = time.Time{}
		} else if parsed, err := time.Parse(time.RFC3339, strings.TrimSpace(*patch.SaleStartsAt)); err != nil {
			fields["sale_starts_at"] = "Use a valid date and time."
		} else {
			p.SaleStartsAt = parsed
		}
	}
	if _, ok := raw["sale_ends_at"]; ok {
		if patch.SaleEndsAt == nil || strings.TrimSpace(*patch.SaleEndsAt) == "" {
			p.SaleEndsAt = time.Time{}
		} else if parsed, err := time.Parse(time.RFC3339, strings.TrimSpace(*patch.SaleEndsAt)); err != nil {
			fields["sale_ends_at"] = "Use a valid date and time."
		} else {
			p.SaleEndsAt = parsed
		}
	}
	if p.SalePriceCents == nil {
		p.SaleStartsAt = time.Time{}
		p.SaleEndsAt = time.Time{}
	}
	if _, ok := raw["size"]; ok {
		if patch.Size == nil {
			nullField("size")
		} else {
			p.Size = strings.TrimSpace(*patch.Size)
		}
	}
	if _, ok := raw["plugin_id"]; ok {
		if patch.PluginID == nil {
			nullField("plugin_id")
		} else {
			p.PluginID = store.NormalizePluginID(strings.TrimSpace(*patch.PluginID))
		}
	}
	if _, ok := raw["category_slug"]; ok {
		if patch.CategorySlug == nil {
			nullField("category_slug")
		} else {
			slug := strings.TrimSpace(*patch.CategorySlug)
			category, err := s.store.CategoryBySlug(r.Context(), slug)
			if err != nil {
				if errors.Is(err, sql.ErrNoRows) {
					fields["category_slug"] = "Choose a category."
				} else {
					writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
					return
				}
			} else {
				if category.PluginID != store.NormalizePluginID(p.PluginID) {
					fields["category_slug"] = "Choose a category that belongs to this product type."
				} else {
					p.Category = category
				}
			}
		}
	}
	if _, ok := raw["concerns"]; ok {
		if patch.Concerns == nil {
			nullField("concerns")
		} else {
			p.Concerns = trimAll(*patch.Concerns)
		}
	}
	if _, ok := raw["ingredients"]; ok {
		if patch.Ingredients == nil {
			nullField("ingredients")
		} else {
			p.Ingredients = trimAll(*patch.Ingredients)
		}
	}
	if _, ok := raw["how_to"]; ok {
		if patch.HowTo == nil {
			nullField("how_to")
		} else {
			p.HowTo = strings.TrimSpace(*patch.HowTo)
		}
	}
	if _, ok := raw["image"]; ok {
		if patch.Image == nil {
			nullField("image")
		} else {
			p.Image = strings.TrimSpace(*patch.Image)
		}
	}
	if _, ok := raw["gallery"]; ok {
		if patch.Gallery == nil {
			nullField("gallery")
		} else {
			p.Gallery = trimAll(*patch.Gallery)
		}
	}
	if _, ok := raw["stock"]; ok {
		if patch.Stock == nil {
			nullField("stock")
		} else {
			p.Stock = *patch.Stock
		}
	}
	if _, ok := raw["featured"]; ok {
		if patch.Featured == nil {
			nullField("featured")
		} else {
			p.Featured = *patch.Featured
		}
	}
	if _, ok := raw["active"]; ok {
		if patch.Active == nil {
			nullField("active")
		} else {
			p.Active = *patch.Active
		}
	}
	if _, ok := raw["attributes"]; ok {
		if patch.Attributes == nil {
			nullField("attributes")
		} else {
			p.Attributes = normalizeAttributes(*patch.Attributes)
		}
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}

	for key, message := range filterFieldProblems(validateProduct(p), raw) {
		fields[key] = message
	}
	if hasAny(raw, "category_slug", "plugin_id") &&
		(p.Category.ID == "" || p.Category.PluginID != store.NormalizePluginID(p.PluginID)) {
		fields["category_slug"] = "Choose a category that belongs to this product type."
	}
	if hasAny(raw, "attributes", "plugin_id") {
		for key, message := range s.validateProductAttributes(p.PluginID, p.Attributes) {
			fields[key] = message
		}
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}

	taken, err := s.slugTaken(r.Context(), p.Slug, p.ID)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	if taken {
		writeProblem(w, problemFields(409, "conflict", "That slug is already in use.",
			map[string]string{"slug": "This slug is already in use."}))
		return
	}

	now := time.Now().UTC()
	p.UpdatedAt = now
	keys := make([]string, 0, len(raw))
	for key := range raw {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	detail, _ := json.Marshal(map[string]any{"slug": p.Slug, "fields": keys})
	actor, _ := currentAuth(r)
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.UpdateProduct(r.Context(), tx, p); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "product.update",
			EntityType:  "product",
			EntityID:    p.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		if isUniqueViolation(err) {
			writeProblem(w, problemFields(409, "conflict", "That slug is already in use.",
				map[string]string{"slug": "This slug is already in use."}))
			return
		}
		s.logger.Error("update product failed", "error", err, "product_id", p.ID, "request_id", requestID(r))
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{"product": s.toProduct(r.Context(), p, true)})
}

func (s *Server) handleArchiveProduct(w http.ResponseWriter, r *http.Request) {
	id := chi.URLParam(r, "id")
	product, err := s.store.ProductByID(r.Context(), id, false)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeError(w, http.StatusNotFound, "not_found", "Product not found.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	now := time.Now().UTC()
	detail, _ := json.Marshal(map[string]string{"slug": product.Slug})
	actor, _ := currentAuth(r)
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.SetProductActive(r.Context(), tx, product.ID, false, now); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "product.archive",
			EntityType:  "product",
			EntityID:    product.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (s *Server) slugTaken(ctx context.Context, slug, exceptID string) (bool, error) {
	product, err := s.store.ProductBySlug(ctx, slug, false)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return false, nil
		}
		return false, err
	}
	return product.ID != exceptID, nil
}

func parseSaleWindow(salePrice *int64, starts, ends string) (*int64, time.Time, time.Time, map[string]string) {
	fields := make(map[string]string)
	starts = strings.TrimSpace(starts)
	ends = strings.TrimSpace(ends)
	if salePrice == nil {
		if starts != "" || ends != "" {
			fields["sale_price_cents"] = "Add a sale price to use a sale window."
		}
		return nil, time.Time{}, time.Time{}, fields
	}
	window := store.SaleWindow{PriceCents: salePrice}
	if *salePrice == 0 {
		return nil, time.Time{}, time.Time{}, fields
	}
	if !window.PricePositive() {
		fields["sale_price_cents"] = "Enter a sale price above 0."
		return nil, time.Time{}, time.Time{}, fields
	}
	value := *salePrice
	start, startErr := time.Parse(time.RFC3339, starts)
	if startErr != nil {
		fields["sale_starts_at"] = "Choose when the sale starts."
	}
	end, endErr := time.Parse(time.RFC3339, ends)
	if endErr != nil {
		fields["sale_ends_at"] = "Choose when the sale ends."
	}
	window = store.SaleWindow{PriceCents: &value, StartsAt: start, EndsAt: end}
	if startErr == nil && endErr == nil && !window.EndsAfterStarts() {
		fields["sale_ends_at"] = "The sale must end after it starts."
	}
	return &value, start, end, fields
}

// A partial patch only validates the fields it actually sends. A stock
// adjustment on a legacy row with an outdated plugin schema must still work;
// the full form validation belongs to a full form save.
func filterFieldProblems(problems map[string]string, raw map[string]json.RawMessage) map[string]string {
	out := make(map[string]string)
	saleTouched := hasAny(raw, "sale_price_cents", "sale_starts_at", "sale_ends_at")
	for key, message := range problems {
		keep := hasAny(raw, key)
		if !keep && strings.HasPrefix(key, "sale_") {
			keep = saleTouched
		}
		if keep {
			out[key] = message
		}
	}
	return out
}

func hasAny(raw map[string]json.RawMessage, keys ...string) bool {
	for _, key := range keys {
		if _, ok := raw[key]; ok {
			return true
		}
	}
	return false
}

func validateProduct(p store.Product) map[string]string {
	fields := make(map[string]string)
	if runeLen(p.Name) < 2 || runeLen(p.Name) > 120 {
		fields["name"] = "Use between 2 and 120 characters."
	}
	if !validSlug(p.Slug) {
		fields["slug"] = "Use lowercase letters, numbers and hyphens."
	}
	if runeLen(p.Subtitle) > 160 {
		fields["subtitle"] = "Keep the subtitle under 160 characters."
	}
	if runeLen(p.Description) > 4000 {
		fields["description"] = "Keep the description under 4000 characters."
	}
	if p.PriceCents <= 0 || p.PriceCents >= 100000000 {
		fields["price_cents"] = "Enter a price above 0 and below 100000000."
	}
	if p.CompareAtCents != nil && (*p.CompareAtCents <= 0 || *p.CompareAtCents >= 100000000) {
		fields["compare_at_cents"] = "Enter a compare-at price above 0, or leave it empty."
	}
	for _, problem := range p.SaleWindow().Problems(p.PriceCents) {
		switch problem {
		case store.SalePriceInvalid:
			fields["sale_price_cents"] = "Enter a sale price above 0 and below the regular price."
		case store.SalePriceMissing:
			fields["sale_price_cents"] = "Add a sale price to use a sale window."
		case store.SaleStartMissing:
			fields["sale_starts_at"] = "Set when the sale starts."
		case store.SaleEndMissing:
			fields["sale_ends_at"] = "Set when the sale ends."
		case store.SaleEndNotAfterStart:
			fields["sale_ends_at"] = "The sale must end after it starts."
		}
	}
	if runeLen(p.Size) > 60 {
		fields["size"] = "Keep the size under 60 characters."
	}
	if len(p.Concerns) > 12 {
		fields["concerns"] = "Use at most 12 concerns."
	} else {
		for _, concern := range p.Concerns {
			if runeLen(concern) > 80 {
				fields["concerns"] = "Keep each concern under 80 characters."
				break
			}
		}
	}
	if len(p.Ingredients) > 12 {
		fields["ingredients"] = "Use at most 12 ingredients."
	} else {
		for _, ingredient := range p.Ingredients {
			if runeLen(ingredient) > 120 {
				fields["ingredients"] = "Keep each ingredient under 120 characters."
				break
			}
		}
	}
	if runeLen(p.HowTo) > 1000 {
		fields["how_to"] = "Keep the instructions under 1000 characters."
	}
	if !validImagePath(p.Image) {
		fields["image"] = "Use an uploaded image."
	}
	if len(p.Gallery) > 12 {
		fields["gallery"] = "Use at most 12 images."
	} else {
		for _, image := range p.Gallery {
			if !validImagePath(image) {
				fields["gallery"] = "Each image must be an uploaded image."
				break
			}
		}
	}
	if p.Stock < 0 || p.Stock > 100000 {
		fields["stock"] = "Use a stock count between 0 and 100000."
	}
	return fields
}
