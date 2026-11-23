package productrules

import "testing"

func BenchmarkValidateProductAttributes(b *testing.B) {
	productType := optionsSchema()
	attributes := validOptions()
	b.ReportAllocs()
	for b.Loop() {
		if problems := ValidateAttributes(productType, attributes); len(problems) != 0 {
			b.Fatalf("fixture is no longer valid: %#v", problems)
		}
	}
}
