// Package orderstatus owns the order lifecycle: the statuses an order can hold
// and the moves allowed between them. The admin API and the codemode planner
// both ask this table instead of carrying their own copy.
package orderstatus

import "slices"

// Statuses lists every status an order can hold, in lifecycle order.
var Statuses = []string{"pending", "confirmed", "packed", "shipped", "delivered", "cancelled"}

// allowed maps a status to the ones it may move to next. Terminal statuses
// carry an empty list.
var allowed = map[string][]string{
	"pending":   {"confirmed", "cancelled"},
	"confirmed": {"packed", "cancelled"},
	"packed":    {"shipped", "cancelled"},
	"shipped":   {"delivered"},
	"delivered": {},
	"cancelled": {},
}

// Known reports whether status is one of Statuses.
func Known(status string) bool {
	return slices.Contains(Statuses, status)
}

// CanTransition reports whether an order may move from one status to another.
func CanTransition(from, to string) bool {
	return slices.Contains(allowed[from], to)
}
