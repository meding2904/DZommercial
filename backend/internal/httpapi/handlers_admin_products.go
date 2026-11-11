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
	Name           string   `json:"name"`
	Slug           string   `json:"slug"`
	Subtitle       string   `json:"subtitle"`
	Description    string   `json:"description"`
	PriceCents     int64    `json:"price_cents"`
	CompareAtCents *int64   `json:"compare_at_cents"`
	Size           string   `json:"size"`
	CategorySlug   string   `json:"category_slug"`
	Concerns       []string `json:"concerns"`
	Ingredients    []string `json:"ingredients"`
	HowTo          string   `json:"how_to"`
	Image          string   `json:"image"`
	Gallery        []string `json:"gallery"`
	Stock          int      `json:"stock"`
	Featured       bool     `json:"featured"`
	Active         *bool    `json:"active"`
}

type productPatch struct {
	Name           *string   `json:"name"`
	Slug           *string   `json:"slug"`
	Subtitle       *string   `json:"subtitle"`
	Description    *string   `json:"description"`
	PriceCents     *int64    `json:"price_cents"`
	CompareAtCents *int64    `json:"compare_at_cents"`
	Size           *string   `json:"size"`
	CategorySlug   *string   `json:"category_slug"`
	Concerns       *[]string `json:"concerns"`
	Ingredients    *[]string `json:"ingredients"`
	HowTo          *string   `json:"how_to"`
	Image          *string   `json:"image"`
	Gallery        *[]string `json:"gallery"`
	Stock          *int      `json:"stock"`
	Featured       *bool     `json:"featured"`
	Active         *bool     `json:"active"`
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
	writeJSON(w, http.StatusOK, map[string]any{"products": toProducts(products, true)})
}

func (s *Server) handleCreateProduct(w http.ResponseWriter, r *http.Request) {
	payload, _, bad := decodeJSON[productCreatePayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	now := time.Now().UTC()
	product := store.Product{
		ID:             ids.New("prd"),
		Name:           strings.TrimSpace(payload.Name),
		Slug:           strings.TrimSpace(payload.Slug),
		Subtitle:       strings.TrimSpace(payload.Subtitle),
		Description:    strings.TrimSpace(payload.Description),
		PriceCents:     payload.PriceCents,
		CompareAtCents: payload.CompareAtCents,
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
		CreatedAt:      now,
		UpdatedAt:      now,
	}
	if payload.Active != nil {
		product.Active = *payload.Active
	}

	fields := validateProduct(product)
	category, err := s.store.CategoryBySlug(r.Context(), strings.TrimSpace(payload.CategorySlug))
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			fields["category_slug"] = "Choose a category."
		} else {
			writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
	} else {
		product.Category = category
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

	writeJSON(w, http.StatusCreated, map[string]any{"product": toProduct(product, true)})
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
	if _, ok := raw["size"]; ok {
		if patch.Size == nil {
			nullField("size")
		} else {
			p.Size = strings.TrimSpace(*patch.Size)
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
				p.Category = category
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
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}

	for key, message := range validateProduct(p) {
		fields[key] = message
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

	writeJSON(w, http.StatusOK, map[string]any{"product": toProduct(p, true)})
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
		fields["image"] = "Use /images/ or an https:// address."
	}
	if len(p.Gallery) > 12 {
		fields["gallery"] = "Use at most 12 images."
	} else {
		for _, image := range p.Gallery {
			if !validImagePath(image) {
				fields["gallery"] = "Each image must use /images/ or an https:// address."
				break
			}
		}
	}
	if p.Stock < 0 || p.Stock > 100000 {
		fields["stock"] = "Use a stock count between 0 and 100000."
	}
	return fields
}
