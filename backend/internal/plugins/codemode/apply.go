package codemode

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"azahar/internal/ids"
	"azahar/internal/plugins"
	"azahar/internal/store"
	"azahar/internal/storefront"
)

var errStalePlan = errors.New("codemode plan is stale")

func (p *Plugin) applyAction(
	ctx context.Context,
	tx *sql.Tx,
	c *plugins.Context,
	planID string,
	action Action,
	theme storefront.Theme,
	baseline map[string]string,
	now time.Time,
) (string, storefront.Theme, error) {
	switch action.Type {
	case "theme.update":
		return p.applyThemeUpdate(ctx, tx, c, planID, action.Patch, theme, now)
	case "product.update":
		message, err := p.applyProductUpdate(ctx, tx, c, planID, action, baseline, now)
		return message, theme, err
	case "product.archive":
		message, err := p.applyProductArchive(ctx, tx, c, planID, action, baseline, now)
		return message, theme, err
	case "product.create":
		message, err := p.applyProductCreate(ctx, tx, c, planID, action, now)
		return message, theme, err
	case "order.status":
		message, err := p.applyOrderStatus(ctx, tx, c, planID, action, baseline, now)
		return message, theme, err
	default:
		return "", theme, fmt.Errorf("unknown action type %q", action.Type)
	}
}

func (p *Plugin) applyThemeUpdate(
	ctx context.Context,
	tx *sql.Tx,
	c *plugins.Context,
	planID string,
	raw json.RawMessage,
	theme storefront.Theme,
	now time.Time,
) (string, storefront.Theme, error) {
	next, err := applyThemePatch(theme, raw)
	if err != nil {
		return "", theme, err
	}
	payload, err := storefront.Encode(next)
	if err != nil {
		return "", theme, errors.New("the theme could not be encoded")
	}
	if err := c.Env.Store.SaveStorefrontSettings(ctx, tx, payload, now); err != nil {
		logWarn(c.Env, "codemode theme could not be saved")
		return "", theme, errors.New("the theme could not be saved")
	}
	if err := insertActionAudit(ctx, tx, c, "codemode.theme_update", "storefront", "default", planID, now); err != nil {
		return "", theme, errors.New("the theme audit entry could not be saved")
	}
	next.UpdatedAt = store.FormatTime(now)
	return "Updated the storefront theme.", next, nil
}

func (p *Plugin) applyProductUpdate(
	ctx context.Context,
	tx *sql.Tx,
	c *plugins.Context,
	planID string,
	action Action,
	baseline map[string]string,
	now time.Time,
) (string, error) {
	patch, err := parseProductPatch(action.Patch)
	if err != nil {
		return "", err
	}
	product, err := c.Env.Store.ProductByIDTx(ctx, tx, action.ProductID, false)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return "", fmt.Errorf("product %q no longer exists", action.ProductID)
		}
		logWarn(c.Env, "codemode product could not be read", "product_id", action.ProductID)
		return "", errors.New("the product could not be read")
	}
	want, ok := baseline["product:"+product.ID]
	if !ok || productFingerprint(product) != want {
		return "", errStalePlan
	}

	if patch.Name != nil {
		product.Name = *patch.Name
	}
	if patch.Subtitle != nil {
		product.Subtitle = *patch.Subtitle
	}
	if patch.Description != nil {
		product.Description = *patch.Description
	}
	if patch.PriceCents != nil {
		product.PriceCents = *patch.PriceCents
	}
	if patch.Stock != nil {
		product.Stock = *patch.Stock
	}
	if patch.Active != nil {
		product.Active = *patch.Active
	}
	if product.SalePriceCents != nil && !product.SaleWindow().PriceValid(product.PriceCents) {
		return "", errors.New("the sale price must stay below the regular price")
	}
	product.UpdatedAt = now

	if err := c.Env.Store.UpdateProduct(ctx, tx, product); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return "", fmt.Errorf("product %q no longer exists", action.ProductID)
		}
		logWarn(c.Env, "codemode product could not be updated", "product_id", action.ProductID)
		return "", errors.New("the product could not be updated")
	}
	baseline["product:"+product.ID] = productFingerprint(product)
	if err := insertActionAudit(ctx, tx, c, "codemode.product_update", "product", product.ID, planID, now); err != nil {
		return "", errors.New("the product audit entry could not be saved")
	}
	return fmt.Sprintf("Updated %q.", product.Name), nil
}

func (p *Plugin) applyProductArchive(
	ctx context.Context,
	tx *sql.Tx,
	c *plugins.Context,
	planID string,
	action Action,
	baseline map[string]string,
	now time.Time,
) (string, error) {
	product, err := c.Env.Store.ProductByIDTx(ctx, tx, action.ProductID, false)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return "", fmt.Errorf("product %q no longer exists", action.ProductID)
		}
		logWarn(c.Env, "codemode product could not be read", "product_id", action.ProductID)
		return "", errors.New("the product could not be read")
	}
	want, ok := baseline["product:"+product.ID]
	if !ok || productFingerprint(product) != want {
		return "", errStalePlan
	}
	if err := c.Env.Store.SetProductActive(ctx, tx, product.ID, false, now); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return "", fmt.Errorf("product %q no longer exists", action.ProductID)
		}
		logWarn(c.Env, "codemode product could not be archived", "product_id", action.ProductID)
		return "", errors.New("the product could not be archived")
	}
	product.Active = false
	baseline["product:"+product.ID] = productFingerprint(product)
	if err := insertActionAudit(ctx, tx, c, "codemode.product_archive", "product", product.ID, planID, now); err != nil {
		return "", errors.New("the product audit entry could not be saved")
	}
	return fmt.Sprintf("Archived %q.", product.Name), nil
}

func (p *Plugin) applyProductCreate(
	ctx context.Context,
	tx *sql.Tx,
	c *plugins.Context,
	planID string,
	action Action,
	now time.Time,
) (string, error) {
	draft := action.Product
	if err := p.validateProductDraft(draft); err != nil {
		return "", err
	}
	category, err := c.Env.Store.CategoryBySlugTx(ctx, tx, draft.CategorySlug)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return "", fmt.Errorf("category %q no longer exists", draft.CategorySlug)
		}
		logWarn(c.Env, "codemode category could not be read")
		return "", errors.New("the category could not be read")
	}
	if category.PluginID != draft.PluginID {
		return "", fmt.Errorf("category %q belongs to another product type", draft.CategorySlug)
	}

	var taken int
	if err := tx.QueryRowContext(ctx, `SELECT COUNT(*) FROM products WHERE slug = ?`, draft.Slug).Scan(&taken); err != nil {
		logWarn(c.Env, "codemode slug could not be checked")
		return "", errors.New("the product slug could not be checked")
	}
	if taken > 0 {
		return "", fmt.Errorf("slug %q is already in use", draft.Slug)
	}

	product := store.Product{
		ID:          ids.New("prd"),
		Slug:        draft.Slug,
		Name:        draft.Name,
		Description: draft.Description,
		PriceCents:  draft.PriceCents,
		Currency:    "DZD",
		Category:    category,
		Image:       draft.Image,
		Stock:       draft.Stock,
		Active:      true,
		PluginID:    draft.PluginID,
		Attributes:  draft.Attributes,
		CreatedAt:   now,
		UpdatedAt:   now,
	}
	if err := c.Env.Store.CreateProduct(ctx, tx, product); err != nil {
		logWarn(c.Env, "codemode product could not be created", "slug", draft.Slug)
		return "", errors.New("the product could not be created")
	}
	if err := insertActionAudit(ctx, tx, c, "codemode.product_create", "product", product.ID, planID, now); err != nil {
		return "", errors.New("the product audit entry could not be saved")
	}
	return fmt.Sprintf("Created %q.", product.Name), nil
}

func (p *Plugin) applyOrderStatus(
	ctx context.Context,
	tx *sql.Tx,
	c *plugins.Context,
	planID string,
	action Action,
	baseline map[string]string,
	now time.Time,
) (string, error) {
	order, err := c.Env.Store.OrderByIDTx(ctx, tx, action.OrderID)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return "", fmt.Errorf("order %q no longer exists", action.OrderID)
		}
		logWarn(c.Env, "codemode order could not be read", "order_id", action.OrderID)
		return "", errors.New("the order could not be read")
	}
	want, ok := baseline["order:"+order.ID]
	if !ok || order.Status != want {
		return "", errStalePlan
	}
	if !canOrderTransition(order.Status, action.Status) {
		return "", fmt.Errorf("order %s cannot move from %s to %s",
			order.Number, order.Status, action.Status)
	}

	if err := c.Env.Store.UpdateOrderStatus(ctx, tx, order.ID, order.Status, action.Status, now); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return "", errStalePlan
		}
		logWarn(c.Env, "codemode order could not be updated", "order_id", order.ID)
		return "", errors.New("the order could not be updated")
	}
	baseline["order:"+order.ID] = action.Status
	if action.Status == "cancelled" {
		items, err := c.Env.Store.OrderItemsTx(ctx, tx, order.ID)
		if err != nil {
			logWarn(c.Env, "codemode order items could not be read", "order_id", order.ID)
			return "", errors.New("the order items could not be read")
		}
		for _, item := range items {
			if err := c.Env.Store.IncrementStock(ctx, tx, item.ProductID, item.Quantity); err != nil {
				logWarn(c.Env, "codemode order stock could not be restored", "order_id", order.ID)
				return "", errors.New("the order stock could not be restored")
			}
			// A later product.update in the same plan must compare against the
			// restocked row, not the pre-cancel snapshot.
			if product, err := c.Env.Store.ProductByIDTx(ctx, tx, item.ProductID, false); err == nil {
				baseline["product:"+item.ProductID] = productFingerprint(product)
			} else if !errors.Is(err, sql.ErrNoRows) {
				logWarn(c.Env, "codemode restocked product could not be read", "product_id", item.ProductID)
				return "", errors.New("the restocked product could not be read")
			}
		}
	}
	if err := insertActionAudit(ctx, tx, c, "codemode.order_status", "order", order.ID, planID, now); err != nil {
		return "", errors.New("the order audit entry could not be saved")
	}
	if p.registry != nil {
		for _, hook := range p.registry.OrderStatusHooks() {
			if err := hook.OnOrderStatusChanged(ctx, tx, order.ID, order.Status, action.Status); err != nil {
				logWarn(c.Env, "codemode order status hook failed", "order_id", order.ID)
				return "", errors.New("the order status could not be completed")
			}
		}
	}
	return fmt.Sprintf("Moved order %s from %s to %s.", order.Number, order.Status, action.Status), nil
}

func insertActionAudit(
	ctx context.Context,
	tx *sql.Tx,
	c *plugins.Context,
	action, entityType, entityID, planID string,
	now time.Time,
) error {
	detail, _ := json.Marshal(map[string]string{"via": "codemode", "plan": planID})
	return c.Env.Store.InsertAudit(ctx, tx, store.AuditEntry{
		ID:          ids.New("aud"),
		ActorUserID: c.UserID,
		Action:      action,
		EntityType:  entityType,
		EntityID:    entityID,
		Detail:      string(detail),
		CreatedAt:   now,
	})
}
