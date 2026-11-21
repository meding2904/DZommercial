package store

import (
	"context"
	"fmt"
	"time"
)

type ProductImage struct {
	ID        string
	MIME      string
	Bytes     []byte
	CreatedBy string
	CreatedAt time.Time
}

func (s *Store) CreateProductImage(ctx context.Context, q querier, image ProductImage) error {
	_, err := q.ExecContext(ctx,
		`INSERT INTO product_images (id, mime, bytes, created_by, created_at)
		 VALUES (?, ?, ?, ?, ?)`,
		image.ID, image.MIME, image.Bytes, image.CreatedBy, FormatTime(image.CreatedAt),
	)
	if err != nil {
		return fmt.Errorf("create product image: %w", err)
	}
	return nil
}

func (s *Store) ProductImage(ctx context.Context, id string) (ProductImage, error) {
	var image ProductImage
	var createdAt string
	err := s.db.QueryRowContext(ctx,
		`SELECT id, mime, bytes, created_by, created_at FROM product_images WHERE id = ?`, id,
	).Scan(&image.ID, &image.MIME, &image.Bytes, &image.CreatedBy, &createdAt)
	if err != nil {
		return ProductImage{}, err
	}
	image.CreatedAt = ParseTime(createdAt)
	return image, nil
}
