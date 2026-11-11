package httpapi

import (
	"database/sql"
	"errors"
	"net/http"
	"strings"

	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

func (s *Server) handleCategories(w http.ResponseWriter, r *http.Request) {
	categories, err := s.store.Categories(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	result := make([]categoryDTO, 0, len(categories))
	for _, category := range categories {
		result = append(result, categoryDTO{
			Slug:        category.Slug,
			Name:        category.Name,
			Description: category.Description,
		})
	}
	writeJSON(w, http.StatusOK, map[string]any{"categories": result})
}

func (s *Server) handleProducts(w http.ResponseWriter, r *http.Request) {
	query := r.URL.Query()
	filter := store.ProductFilter{
		Category: strings.TrimSpace(query.Get("category")),
		Concern:  strings.TrimSpace(query.Get("concern")),
		Query:    strings.TrimSpace(query.Get("q")),
		Sort:     query.Get("sort"),
	}
	if runeLen(filter.Query) > 100 {
		filter.Query = string([]rune(filter.Query)[:100])
	}
	products, err := s.store.ListProducts(r.Context(), filter)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"products": toProducts(products, false)})
}

func (s *Server) handleProduct(w http.ResponseWriter, r *http.Request) {
	slug := chi.URLParam(r, "slug")
	product, err := s.store.ProductBySlug(r.Context(), slug, true)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeError(w, http.StatusNotFound, "not_found", "Product not found.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	related, err := s.store.RelatedProducts(r.Context(), product.Category.ID, product.ID, 4)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"product": toProduct(product, false),
		"related": toProducts(related, false),
	})
}
