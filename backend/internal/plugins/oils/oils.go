// Package oils contributes the skincare product type: a single formula with
// its size, concerns, materials and instructions. It is a plugin so the core
// catalog stays free of any one trade's vocabulary.
package oils

import "azahar/internal/plugins"

const pluginID = "oils"

var _ plugins.Plugin = (*Plugin)(nil)

type Plugin struct{}

func New() *Plugin {
	return &Plugin{}
}

func (p *Plugin) Manifest() plugins.Manifest {
	return plugins.Manifest{
		ID:          pluginID,
		Name:        "Oil shelf",
		Version:     "1.0.0",
		Author:      "Azahar",
		Description: "Sells a single formula with its size, concerns, materials and instructions.",
		AdminPath:   "/admin/plugins/oils",
		Categories: []plugins.Category{
			{Slug: "face", Name: "Face", Description: "Oils, serums and masks for the face.", SortOrder: 1},
			{Slug: "body", Name: "Body", Description: "Balms, oils and creams for the body.", SortOrder: 2},
			{Slug: "ritual", Name: "Ritual", Description: "Soaks for the bath.", SortOrder: 3},
		},
		ProductType: &plugins.ProductType{
			Label:   "Oil",
			Summary: "A formula with its size, concerns, materials and instructions.",
			Fields: []plugins.OptionField{
				{
					Key:      "size",
					Label:    "Size",
					Type:     "text",
					Required: true,
					Help:     "The volume or weight shown on the product page, for example 100 ml.",
				},
				{
					Key:         "concerns",
					Label:       "Concerns",
					Type:        "list",
					Required:    false,
					Help:        "The skin concerns this formula answers. The shop filter reads these.",
					Suggestions: []string{"dry", "sensitive", "dull", "oily", "tired"},
				},
				{
					Key:      "ingredients",
					Label:    "Ingredients",
					Type:     "list",
					Required: false,
					Help:     "The materials that carry the formula.",
				},
				{
					Key:      "how_to",
					Label:    "How to use",
					Type:     "textarea",
					Required: false,
					Help:     "One short paragraph: how much, where and when.",
				},
			},
		},
	}
}

func (p *Plugin) Routes() []plugins.Route {
	return nil
}
