package pcbuilder

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	"azahar/internal/plugins"
	"azahar/internal/plugins/pcparts"
	"azahar/internal/store"
)

func TestManifestDeclaresStorefrontPage(t *testing.T) {
	manifest := New().Manifest()
	if manifest.ID != "pcbuilder" {
		t.Fatalf("id = %q, want pcbuilder", manifest.ID)
	}
	if len(manifest.Dependencies) != 1 || manifest.Dependencies[0] != "pcparts" {
		t.Fatalf("dependencies = %#v, want [pcparts]", manifest.Dependencies)
	}
	if len(manifest.StorefrontPages) != 1 {
		t.Fatalf("storefront pages = %d, want 1", len(manifest.StorefrontPages))
	}
	page := manifest.StorefrontPages[0]
	if page.Path != "/build-your-pc" || page.Label != "Build your PC" || page.SortOrder != 10 {
		t.Fatalf("storefront page = %+v, want /build-your-pc Build your PC at 10", page)
	}
}

func TestRoutesExposeParts(t *testing.T) {
	routes := New().Routes()
	if len(routes) != 1 {
		t.Fatalf("routes = %d, want 1", len(routes))
	}
	if routes[0].Method != http.MethodGet || routes[0].Pattern != "/parts" {
		t.Fatalf("route = %s %s, want GET /parts", routes[0].Method, routes[0].Pattern)
	}
	if routes[0].Admin {
		t.Fatal("the parts route must be public")
	}
	if routes[0].Handler == nil {
		t.Fatal("the parts route needs a handler")
	}
}

func TestRegistration(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(pcparts.New()); err != nil {
		t.Fatalf("register pcparts: %v", err)
	}
	if err := registry.Register(New()); err != nil {
		t.Fatalf("register pcbuilder: %v", err)
	}
}

func TestPartsHandlerWithoutStore(t *testing.T) {
	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/api/plugins/pcbuilder/parts", nil)
	New().handleParts(&plugins.Context{
		W:   recorder,
		R:   request,
		Env: &plugins.Env{},
	})
	if recorder.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusServiceUnavailable)
	}
}

func TestPartsResponseShape(t *testing.T) {
	categories := []store.Category{
		{Slug: "memory", Name: "Memory", SortOrder: 4, PluginID: pcparts.PluginID},
		{Slug: "cpus", Name: "CPU", SortOrder: 1, PluginID: pcparts.PluginID},
		{Slug: "cases", Name: "Cases", SortOrder: 7, PluginID: pcparts.PluginID},
		{Slug: "face", Name: "Face", SortOrder: 1, PluginID: "oils"},
	}
	products := []store.Product{
		testPart("cpu_ryzen", "Ryzen 9", "cpus", "CPU"),
		testPart("cpu_athlon", "Athlon", "cpus", "CPU"),
		testPart("memory_ddr5", "DDR5 kit", "memory", "Memory"),
	}
	inactive := testPart("cpu_old", "Old CPU", "cpus", "CPU")
	inactive.Active = false
	products = append(products, inactive)
	foreign := testPart("face_oil", "Face oil", "face", "Face")
	foreign.PluginID = "oils"
	products = append(products, foreign)

	response := buildPartsResponse(categories, products)
	if len(response.Categories) != 3 {
		t.Fatalf("categories = %d, want 3", len(response.Categories))
	}
	if response.Categories[0].Slug != "cpus" ||
		response.Categories[1].Slug != "memory" ||
		response.Categories[2].Slug != "cases" {
		t.Fatalf("category order = %s, %s, %s",
			response.Categories[0].Slug, response.Categories[1].Slug, response.Categories[2].Slug)
	}
	cpus := response.Categories[0].Products
	if len(cpus) != 2 {
		t.Fatalf("cpus = %d, want 2 (inactive and foreign products excluded)", len(cpus))
	}
	if cpus[0].Name != "Athlon" || cpus[1].Name != "Ryzen 9" {
		t.Fatalf("cpu order = %q, %q, want Athlon then Ryzen 9", cpus[0].Name, cpus[1].Name)
	}
	if len(response.Categories[2].Products) != 0 {
		t.Fatalf("empty case category has %d products, want 0", len(response.Categories[2].Products))
	}

	raw, err := json.Marshal(response)
	if err != nil {
		t.Fatalf("marshal response: %v", err)
	}
	var decoded struct {
		Categories []struct {
			Slug     string                       `json:"slug"`
			Name     string                       `json:"name"`
			Products []map[string]json.RawMessage `json:"products"`
		} `json:"categories"`
	}
	if err := json.Unmarshal(raw, &decoded); err != nil {
		t.Fatalf("unmarshal response: %v", err)
	}
	product := decoded.Categories[0].Products[0]
	for _, key := range []string{
		"id", "slug", "name", "subtitle", "description", "price_cents", "currency",
		"image", "stock", "plugin_id", "attributes", "category",
	} {
		if _, ok := product[key]; !ok {
			t.Fatalf("product JSON is missing %q: %s", key, raw)
		}
	}
	var attributes map[string][]string
	if err := json.Unmarshal(product["attributes"], &attributes); err != nil {
		t.Fatalf("attributes: %v", err)
	}
	if len(attributes["socket"]) != 1 || attributes["socket"][0] != "AM5" {
		t.Fatalf("attributes.socket = %v, want [AM5]", attributes["socket"])
	}
}

func TestPartsResponseStopsAtFiveHundred(t *testing.T) {
	categories := []store.Category{
		{Slug: "cpus", Name: "CPU", SortOrder: 1, PluginID: pcparts.PluginID},
	}
	products := make([]store.Product, 0, 501)
	for index := 0; index < 501; index++ {
		products = append(products, testPart(
			fmt.Sprintf("cpu_%03d", index),
			fmt.Sprintf("CPU %03d", index),
			"cpus", "CPU",
		))
	}
	response := buildPartsResponse(categories, products)
	if got := len(response.Categories[0].Products); got != 500 {
		t.Fatalf("products = %d, want 500", got)
	}
}

func testPart(id, name, categorySlug, categoryName string) store.Product {
	return store.Product{
		ID:       id,
		Slug:     id,
		Name:     name,
		Active:   true,
		PluginID: pcparts.PluginID,
		Category: store.Category{
			Slug:     categorySlug,
			Name:     categoryName,
			PluginID: pcparts.PluginID,
		},
		Attributes: map[string][]string{"socket": {"AM5"}},
	}
}
