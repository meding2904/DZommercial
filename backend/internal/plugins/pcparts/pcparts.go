// Package pcparts contributes the PC parts product type and the shelves a
// machine is assembled from: processors, boards, memory, drives, power,
// cases and cooling. Every part is described with the same set of optional
// text attributes so the builder can compare them without knowing which
// category a product belongs to.
package pcparts

import "azahar/internal/plugins"

// PluginID is used by the PC builder to read only these products.
const PluginID = "pcparts"

var _ plugins.Plugin = (*Plugin)(nil)

type Plugin struct{}

func New() *Plugin {
	return &Plugin{}
}

func (p *Plugin) Manifest() plugins.Manifest {
	return plugins.Manifest{
		ID:          PluginID,
		Name:        "PC parts",
		Version:     "1.0.0",
		Author:      "Azahar",
		Description: "Sells the parts a desktop PC is built from, with the specs the builder checks.",
		Categories: []plugins.Category{
			{
				Slug:        "cpus",
				Name:        "CPU",
				Description: "Processors with their socket and power draw.",
				SortOrder:   1,
			},
			{
				Slug:        "gpus",
				Name:        "Graphics cards",
				Description: "Graphics cards with their length and power draw.",
				SortOrder:   2,
			},
			{
				Slug:        "motherboards",
				Name:        "Motherboards",
				Description: "Boards with their socket, memory type and form factor.",
				SortOrder:   3,
			},
			{
				Slug:        "memory",
				Name:        "Memory",
				Description: "Memory kits matched to the board's memory type.",
				SortOrder:   4,
			},
			{
				Slug:        "storage",
				Name:        "Storage",
				Description: "Drives with their interface and capacity.",
				SortOrder:   5,
			},
			{
				Slug:        "psus",
				Name:        "Power supplies",
				Description: "Power supplies with their rated wattage.",
				SortOrder:   6,
			},
			{
				Slug:        "cases",
				Name:        "Cases",
				Description: "Cases with their board support and graphics card clearance.",
				SortOrder:   7,
			},
			{
				Slug:        "cooling",
				Name:        "Cooling",
				Description: "Air and liquid coolers with the sockets they fit.",
				SortOrder:   8,
			},
		},
		ProductType: &plugins.ProductType{
			Label:   "PC part",
			Summary: "A part described by the specs the builder needs to compare it.",
			Fields: []plugins.OptionField{
				{
					Key:      "socket",
					Label:    "Socket",
					Type:     "text",
					Required: false,
					Help:     "The CPU socket, for example AM5 or LGA1700. A cooler lists every socket it fits.",
				},
				{
					Key:      "memory_type",
					Label:    "Memory type",
					Type:     "text",
					Required: false,
					Help:     "The memory generation, for example DDR4 or DDR5.",
				},
				{
					Key:      "form_factor",
					Label:    "Form factor",
					Type:     "text",
					Required: false,
					Help:     "The board size, for example ATX, micro-ATX or mini-ITX.",
				},
				{
					Key:      "interface",
					Label:    "Interface",
					Type:     "text",
					Required: false,
					Help:     "The connection a drive uses, for example SATA or NVMe.",
				},
				{
					Key:      "tdp_watts",
					Label:    "TDP (watts)",
					Type:     "text",
					Required: false,
					Help:     "The heat a CPU or graphics card is rated to produce.",
				},
				{
					Key:      "wattage_watts",
					Label:    "Wattage (watts)",
					Type:     "text",
					Required: false,
					Help:     "The rated output of a power supply.",
				},
				{
					Key:      "length_mm",
					Label:    "Length (mm)",
					Type:     "text",
					Required: false,
					Help:     "The length of a graphics card.",
				},
				{
					Key:      "max_gpu_length_mm",
					Label:    "Maximum GPU length (mm)",
					Type:     "text",
					Required: false,
					Help:     "The longest graphics card the case accepts.",
				},
				{
					Key:      "supported_form_factor",
					Label:    "Supported board sizes",
					Type:     "text",
					Required: false,
					Help:     "Comma-separated board sizes a case accepts, for example ATX, micro-ATX, mini-ITX.",
				},
			},
		},
	}
}

func (p *Plugin) Routes() []plugins.Route {
	return nil
}
