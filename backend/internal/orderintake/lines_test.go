package orderintake

import "testing"

func TestBuildLinesMergesRepeatedLines(t *testing.T) {
	first := int64(123400)
	lines, fields := BuildLines([]Line{
		{ProductID: " prd_1 ", Quantity: 1, Options: map[string]string{"color": " Silver "}, ExpectedPriceCents: &first},
		{ProductID: "prd_1", Quantity: 2, Options: map[string]string{"color": "Silver"}},
		{ProductID: "prd_2", Quantity: 4},
	})
	if len(fields) != 0 {
		t.Fatalf("unexpected fields: %#v", fields)
	}
	if len(lines) != 2 {
		t.Fatalf("lines = %d, want 2", len(lines))
	}
	if lines[0].ProductID != "prd_1" || lines[0].Quantity != 3 {
		t.Fatalf("merged line = %#v, want prd_1 × 3", lines[0])
	}
	if lines[0].Options["color"] != "Silver" {
		t.Fatalf("options were not trimmed: %#v", lines[0].Options)
	}
	if lines[0].ExpectedPriceCents == nil || *lines[0].ExpectedPriceCents != first {
		t.Fatalf("the first price pin must survive the merge: %#v", lines[0].ExpectedPriceCents)
	}
	if lines[1].ProductID != "prd_2" || lines[1].Quantity != 4 {
		t.Fatalf("second line = %#v, want prd_2 × 4", lines[1])
	}
}

func TestBuildLinesRejectsBadLines(t *testing.T) {
	cases := []struct {
		name string
		item Line
		want string
	}{
		{"empty product", Line{ProductID: "   ", Quantity: 1}, "Choose a product for every line."},
		{"zero quantity", Line{ProductID: "prd_1", Quantity: 0}, "Use between 1 and 10 units per product."},
		{"too many units", Line{ProductID: "prd_1", Quantity: 11}, "Use between 1 and 10 units per product."},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			lines, fields := BuildLines([]Line{test.item})
			if len(lines) != 0 {
				t.Fatalf("lines = %#v, want none", lines)
			}
			if fields["items"] != test.want {
				t.Fatalf("items message = %q, want %q", fields["items"], test.want)
			}
		})
	}
}

func TestBuildLinesCapsAMergedLine(t *testing.T) {
	lines, fields := BuildLines([]Line{
		{ProductID: "prd_1", Quantity: 8},
		{ProductID: "prd_1", Quantity: 8},
	})
	if len(lines) != 1 || lines[0].Quantity != 16 {
		t.Fatalf("lines = %#v, want one line of 16", lines)
	}
	if fields["items"] != "Use between 1 and 10 units per product." {
		t.Fatalf("items message = %q, want the quantity cap", fields["items"])
	}
}

func TestBuildLinesKeepsAnInvalidLineOutOfTheBasket(t *testing.T) {
	lines, fields := BuildLines([]Line{
		{ProductID: "prd_1", Quantity: 1},
		{ProductID: "", Quantity: 1},
		{ProductID: "prd_2", Quantity: 2},
	})
	if len(lines) != 2 {
		t.Fatalf("lines = %d, want the two valid lines", len(lines))
	}
	if fields["items"] == "" {
		t.Fatal("the invalid line must be reported")
	}
}
