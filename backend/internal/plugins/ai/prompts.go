package ai

import (
	"fmt"
	"sort"
	"strings"

	"azahar/internal/store"
)

func analysisInstruction(language string) string {
	name, ok := supportedLanguages[language]
	if !ok {
		name = supportedLanguages[defaultLanguage]
	}
	return "Use only the facts below. Do not invent numbers or claims. " +
		"Plain text, 120 to 180 words, no markdown headings. Write the answer in " + name + "."
}

// buildStorePrompt includes aggregate facts only. Customer details and order
// identifiers never enter the prompt.
func buildStorePrompt(language string, days int, metrics store.Metrics) (string, string) {
	var b strings.Builder

	fmt.Fprintf(&b, "Period: last %d days (%s to %s).\n",
		days, metrics.From.Format("2006-01-02"), metrics.To.Format("2006-01-02"))
	fmt.Fprintf(&b, "Revenue: %s.\n", formatDZD(metrics.RevenueCents))
	fmt.Fprintf(&b, "Previous period revenue: %s.\n", formatDZD(metrics.PreviousRevenueCents))
	fmt.Fprintf(&b, "Orders: %d.\n", metrics.OrdersCount)
	fmt.Fprintf(&b, "Units sold: %d.\n", metrics.UnitsSold)
	fmt.Fprintf(&b, "New customers: %d.\n", metrics.NewCustomers)

	b.WriteString("Top products by units:\n")
	if len(metrics.TopProducts) == 0 {
		b.WriteString("- None.\n")
	} else {
		for index, item := range metrics.TopProducts {
			if index >= 5 {
				break
			}
			fmt.Fprintf(&b, "- %s: %d units, %s revenue.\n",
				item.Name, item.Units, formatDZD(item.RevenueCents))
		}
	}

	b.WriteString("Low stock:\n")
	if len(metrics.LowStock) == 0 {
		b.WriteString("- None.\n")
	} else {
		for index, item := range metrics.LowStock {
			if index >= 5 {
				break
			}
			fmt.Fprintf(&b, "- %s: %d in stock.\n", item.Name, item.Stock)
		}
	}

	b.WriteString("Order status split:\n")
	if len(metrics.StatusSplit) == 0 {
		b.WriteString("- None.\n")
	} else {
		statuses := make([]string, 0, len(metrics.StatusSplit))
		for status := range metrics.StatusSplit {
			statuses = append(statuses, status)
		}
		sort.Strings(statuses)
		for _, status := range statuses {
			fmt.Fprintf(&b, "- %s: %d orders.\n", status, metrics.StatusSplit[status])
		}
	}

	return analysisInstruction(language), b.String()
}

type productSales struct {
	Days         int
	Units        int
	Orders       int
	RevenueCents int64
}

func buildProductPrompt(language string, product store.Product, sales productSales) (string, string) {
	var b strings.Builder

	fmt.Fprintf(&b, "Product name: %s\n", product.Name)
	if subtitle := strings.TrimSpace(product.Subtitle); subtitle != "" {
		fmt.Fprintf(&b, "Subtitle: %s\n", subtitle)
	}
	if description := strings.TrimSpace(product.Description); description != "" {
		fmt.Fprintf(&b, "Description: %s\n", description)
	}
	fmt.Fprintf(&b, "Category: %s\n", product.Category.Name)
	fmt.Fprintf(&b, "Price: %s\n", formatDZD(product.PriceCents))
	if size := strings.TrimSpace(product.Size); size != "" {
		fmt.Fprintf(&b, "Size: %s\n", size)
	}
	fmt.Fprintf(&b, "Stock: %d\n", product.Stock)
	if sales.Units == 0 && sales.Orders == 0 {
		fmt.Fprintf(&b, "Sales, last %d days: none recorded.\n", sales.Days)
	} else {
		fmt.Fprintf(&b, "Sales, last %d days: %d units, %s revenue, %d orders.\n",
			sales.Days, sales.Units, formatDZD(sales.RevenueCents), sales.Orders)
	}
	if len(product.Concerns) > 0 {
		fmt.Fprintf(&b, "Concerns: %s\n", strings.Join(product.Concerns, ", "))
	}
	if len(product.Ingredients) > 0 {
		fmt.Fprintf(&b, "Ingredients: %s\n", strings.Join(product.Ingredients, ", "))
	}
	if len(product.Attributes) > 0 {
		b.WriteString("Options:\n")
		keys := make([]string, 0, len(product.Attributes))
		for key := range product.Attributes {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		for _, key := range keys {
			values := product.Attributes[key]
			if len(values) == 0 {
				continue
			}
			fmt.Fprintf(&b, "- %s: %s\n", key, strings.Join(values, ", "))
		}
	}

	return analysisInstruction(language), b.String()
}

func formatDZD(cents int64) string {
	sign := ""
	if cents < 0 {
		sign = "-"
		cents = -cents
	}
	return fmt.Sprintf("DZD %s%d.%02d", sign, cents/100, cents%100)
}
