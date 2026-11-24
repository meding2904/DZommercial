//go:build oracle

// Oracle-only exports for the pcbuilder differential test.
package pcbuilder

import "azahar/internal/store"

func OracleBuildPartsResponse(categories []store.Category, products []store.Product) any {
	return buildPartsResponse(categories, products)
}
