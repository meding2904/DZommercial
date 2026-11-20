package httpapi

import (
	"database/sql"
	"errors"
	"io"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"time"

	"azahar/internal/ids"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

const maxProductImageBytes = 5 << 20

var imageIDPattern = regexp.MustCompile(`^img_[a-f0-9]{24}$`)

func (s *Server) handleAdminProductImage(w http.ResponseWriter, r *http.Request) {
	r.Body = http.MaxBytesReader(w, r.Body, maxProductImageBytes+(1<<20))
	if err := r.ParseMultipartForm(maxProductImageBytes); err != nil {
		var maxErr *http.MaxBytesError
		if errors.As(err, &maxErr) {
			writeError(w, http.StatusRequestEntityTooLarge, "payload_too_large", "The image is too large. Use 5 MB or less.")
			return
		}
		writeError(w, http.StatusUnprocessableEntity, "invalid_upload", "Send the image as multipart form data.")
		return
	}
	if r.MultipartForm != nil {
		defer r.MultipartForm.RemoveAll()
	}
	file, _, err := r.FormFile("image")
	if err != nil {
		writeError(w, http.StatusUnprocessableEntity, "invalid_upload", "Choose an image to upload.")
		return
	}
	defer file.Close()

	body, err := io.ReadAll(io.LimitReader(file, maxProductImageBytes+1))
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid_upload", "The image could not be read.")
		return
	}
	if len(body) > maxProductImageBytes {
		writeError(w, http.StatusRequestEntityTooLarge, "payload_too_large", "The image is too large. Use 5 MB or less.")
		return
	}
	mime := http.DetectContentType(body)
	if mime != "image/jpeg" && mime != "image/png" && mime != "image/webp" {
		writeError(w, http.StatusUnprocessableEntity, "invalid_image", "Use a JPEG, PNG or WebP image.")
		return
	}

	now := time.Now().UTC()
	imageID := ids.New("img")
	actor, _ := currentAuth(r)
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.CreateProductImage(r.Context(), tx, store.ProductImage{
			ID:        imageID,
			MIME:      mime,
			Bytes:     body,
			CreatedBy: actor.User.ID,
			CreatedAt: now,
		}); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "product_image.create",
			EntityType:  "product_image",
			EntityID:    imageID,
			Detail:      `{"mime":"` + mime + `"}`,
			CreatedAt:   now,
		})
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "The image could not be saved.")
		return
	}

	writeJSON(w, http.StatusCreated, map[string]any{
		"image": map[string]any{
			"id":    imageID,
			"url":   "/api/images/" + imageID,
			"mime":  mime,
			"bytes": len(body),
		},
	})
}

func (s *Server) handleProductImage(w http.ResponseWriter, r *http.Request) {
	id := strings.TrimSpace(chi.URLParam(r, "id"))
	if !imageIDPattern.MatchString(id) {
		writeError(w, http.StatusNotFound, "not_found", "Image not found.")
		return
	}
	image, err := s.store.ProductImage(r.Context(), id)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeError(w, http.StatusNotFound, "not_found", "Image not found.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	w.Header().Set("Content-Type", image.MIME)
	w.Header().Set("Content-Length", strconv.Itoa(len(image.Bytes)))
	w.Header().Set("Content-Disposition", "inline")
	w.Header().Set("Cache-Control", "public, max-age=31536000, immutable")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(image.Bytes)
}
