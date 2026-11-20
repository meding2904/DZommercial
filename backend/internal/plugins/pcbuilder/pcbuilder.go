// Package pcbuilder adds the public Build your PC page. It reads the parts
// shelf contributed by the pcparts plugin and serves the compact catalog the
// builder needs; the compatibility checks themselves live in the storefront.
package pcbuilder

import (
	"net/http"

	"azahar/internal/plugins"
)

const pluginID = "pcbuilder"

var _ plugins.Plugin = (*Plugin)(nil)

type Plugin struct{}

func New() *Plugin {
	return &Plugin{}
}

func (p *Plugin) Manifest() plugins.Manifest {
	return plugins.Manifest{
		ID:           pluginID,
		Name:         "PC builder",
		Version:      "1.0.0",
		Author:       "Azahar",
		Description:  "Lets a customer choose compatible PC parts and add the whole build to the bag.",
		Dependencies: []string{"pcparts"},
		StorefrontPages: []plugins.StorefrontPage{
			{Path: "/build-your-pc", Label: "Build your PC", SortOrder: 10},
		},
	}
}

func (p *Plugin) Routes() []plugins.Route {
	return []plugins.Route{
		{Method: http.MethodGet, Pattern: "/parts", Handler: p.handleParts},
	}
}
