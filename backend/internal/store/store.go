package store

import (
	"context"
	"database/sql"
	"fmt"
)

type Store struct {
	db *sql.DB
}

func New(handle *sql.DB) *Store {
	return &Store{db: handle}
}

func (s *Store) DB() *sql.DB {
	return s.db
}

func (s *Store) InTx(ctx context.Context, fn func(*sql.Tx) error) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin transaction: %w", err)
	}
	if err := fn(tx); err != nil {
		tx.Rollback()
		return err
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit transaction: %w", err)
	}
	return nil
}

func scanProduct(row scanner) (Product, error) {
	var p Product
	var compareAt sql.NullInt64
	var concerns, ingredients, gallery string
	var featured, active int
	var createdAt, updatedAt string

	err := row.Scan(
		&p.ID, &p.Slug, &p.Name, &p.Subtitle, &p.Description,
		&p.PriceCents, &compareAt, &p.Currency, &p.Size,
		&concerns, &ingredients, &p.HowTo, &p.Image, &gallery,
		&p.Stock, &featured, &active, &createdAt, &updatedAt,
		&p.Category.ID, &p.Category.Slug, &p.Category.Name, &p.Category.Description,
	)
	if err != nil {
		return Product{}, err
	}
	if compareAt.Valid {
		value := compareAt.Int64
		p.CompareAtCents = &value
	}
	p.Concerns = decodeJSON(concerns)
	p.Ingredients = decodeJSON(ingredients)
	p.Gallery = decodeJSON(gallery)
	p.Featured = featured != 0
	p.Active = active != 0
	p.CreatedAt = ParseTime(createdAt)
	p.UpdatedAt = ParseTime(updatedAt)
	return p, nil
}

const productColumns = `p.id, p.slug, p.name, p.subtitle, p.description,
	p.price_cents, p.compare_at_cents, p.currency, p.size,
	p.concerns, p.ingredients, p.how_to, p.image, p.gallery,
	p.stock, p.featured, p.active, p.created_at, p.updated_at,
	c.id, c.slug, c.name, c.description`

func scanOrder(row scanner) (Order, error) {
	var o Order
	var userID sql.NullString
	var wilayaCode sql.NullInt64
	var createdAt, updatedAt string
	var courierEvents, courierSyncedAt string
	err := row.Scan(
		&o.ID, &o.Number, &userID, &o.Name, &o.Email, &o.Phone,
		&o.Address, &o.City, &o.Notes, &o.SubtotalCents,
		&o.ShippingCents, &o.TotalCents, &o.Currency, &o.Status,
		&o.TokenHash, &createdAt, &updatedAt,
		&wilayaCode, &o.WilayaName, &o.CommuneName, &o.DeliveryType,
		&o.StopDeskID, &o.StopDeskName, &o.CourierKey, &o.TrackingNumber,
		&o.CourierStatus, &courierEvents, &courierSyncedAt,
	)
	if err != nil {
		return Order{}, err
	}
	if userID.Valid {
		value := userID.String
		o.UserID = &value
	}
	if wilayaCode.Valid {
		o.WilayaCode = int(wilayaCode.Int64)
	}
	o.CourierEvents = decodeCourierEvents(courierEvents)
	o.CourierSyncedAt = ParseTime(courierSyncedAt)
	o.CreatedAt = ParseTime(createdAt)
	o.UpdatedAt = ParseTime(updatedAt)
	return o, nil
}

const orderColumns = `o.id, o.number, o.user_id, o.name, o.email, o.phone,
	o.address, o.city, o.notes, o.subtotal_cents, o.shipping_cents,
	o.total_cents, o.currency, o.status, o.token_hash, o.created_at, o.updated_at,
	o.wilaya_code, o.wilaya_name, o.commune_name, o.delivery_type,
	o.stop_desk_id, o.stop_desk_name, o.courier_key, o.tracking_number,
	o.courier_status, o.courier_events, o.courier_synced_at`
