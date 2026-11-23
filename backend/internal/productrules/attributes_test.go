package productrules

import (
	"reflect"
	"strings"
	"testing"

	"azahar/internal/plugins"
)

func optionsSchema() *plugins.ProductType {
	return &plugins.ProductType{
		Label: "Oils",
		Fields: []plugins.OptionField{
			{Key: "storage", Label: "Storage", Type: "select", Required: true},
			{Key: "size", Label: "Size", Type: "text"},
			{Key: "notes", Label: "Notes", Type: "textarea"},
			{Key: "concerns", Label: "Concerns", Type: "list"},
		},
	}
}

func validOptions() map[string][]string {
	return map[string][]string{
		"storage":  {"128 GB"},
		"size":     {"100 ml"},
		"notes":    {"Apply daily."},
		"concerns": {"dry", "sensitive"},
	}
}

func TestValidateAttributesAcceptsValidOptions(t *testing.T) {
	if problems := ValidateAttributes(optionsSchema(), validOptions()); len(problems) != 0 {
		t.Fatalf("valid options reported problems: %#v", problems)
	}
}

func TestValidateAttributesReportsProblems(t *testing.T) {
	cases := []struct {
		name       string
		attributes map[string][]string
		want       []Problem
	}{
		{
			name:       "missing required field",
			attributes: map[string][]string{"size": {"100 ml"}},
			want:       []Problem{{Field: "storage", Kind: KindMissingRequired, Label: "Storage"}},
		},
		{
			name:       "two values on a text field",
			attributes: map[string][]string{"storage": {"128 GB"}, "size": {"100 ml", "50 ml"}},
			want:       []Problem{{Field: "size", Kind: KindSingleValue, Label: "Size"}},
		},
		{
			name:       "control rune on a text field",
			attributes: map[string][]string{"storage": {"128 GB"}, "size": {"bell\x07"}},
			want:       []Problem{{Field: "size", Kind: KindInvalidText, Label: "Size", Limit: 120}},
		},
		{
			name:       "textarea over its limit",
			attributes: map[string][]string{"storage": {"128 GB"}, "notes": {strings.Repeat("a", 2001)}},
			want:       []Problem{{Field: "notes", Kind: KindInvalidText, Label: "Notes", Limit: 2000}},
		},
		{
			name: "too many list values",
			attributes: map[string][]string{"storage": {"128 GB"}, "concerns": {
				"a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
			}},
			want: []Problem{{Field: "concerns", Kind: KindTooManyValues, Label: "Concerns"}},
		},
		{
			name:       "duplicate list value",
			attributes: map[string][]string{"storage": {"128 GB"}, "concerns": {"dry", " dry "}},
			want:       []Problem{{Field: "concerns", Kind: KindDuplicateValue, Label: "Concerns"}},
		},
		{
			name:       "list value over its limit",
			attributes: map[string][]string{"storage": {"128 GB"}, "concerns": {strings.Repeat("a", 81)}},
			want:       []Problem{{Field: "concerns", Kind: KindInvalidValue, Label: "Concerns"}},
		},
		{
			name:       "empty list value",
			attributes: map[string][]string{"storage": {"128 GB"}, "concerns": {"  "}},
			want:       []Problem{{Field: "concerns", Kind: KindInvalidValue, Label: "Concerns"}},
		},
		{
			name:       "unknown option",
			attributes: map[string][]string{"storage": {"128 GB"}, "battery": {"5000 mAh"}},
			want:       []Problem{{Kind: KindUnknownField}},
		},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			got := ValidateAttributes(optionsSchema(), testCase.attributes)
			if !reflect.DeepEqual(got, testCase.want) {
				t.Fatalf("problems = %#v, want %#v", got, testCase.want)
			}
		})
	}
}

func TestValidateAttributesTrimsValuesBeforeChecking(t *testing.T) {
	attributes := map[string][]string{"storage": {"  128 GB  "}, "size": {"   "}}
	got := ValidateAttributes(optionsSchema(), attributes)
	want := []Problem{{Field: "size", Kind: KindInvalidText, Label: "Size", Limit: 120}}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("problems = %#v, want %#v", got, want)
	}
}

func TestNormalizeAttributesTrimsKeysAndValues(t *testing.T) {
	normalized, problems := NormalizeAttributes(map[string][]string{
		" storage ": {" 128 GB ", "256 GB"},
	})
	if len(problems) != 0 {
		t.Fatalf("unexpected problems: %#v", problems)
	}
	want := map[string][]string{"storage": {"128 GB", "256 GB"}}
	if !reflect.DeepEqual(normalized, want) {
		t.Fatalf("normalized = %#v, want %#v", normalized, want)
	}
}

func TestNormalizeAttributesReturnsAnEmptyMap(t *testing.T) {
	normalized, problems := NormalizeAttributes(nil)
	if normalized == nil || len(normalized) != 0 || len(problems) != 0 {
		t.Fatalf("normalized = %#v, problems = %#v", normalized, problems)
	}
}

func TestNormalizeAttributesReportsBadKeys(t *testing.T) {
	cases := []struct {
		name       string
		attributes map[string][]string
		want       []Problem
	}{
		{
			name:       "empty key",
			attributes: map[string][]string{"  ": {"128 GB"}},
			want:       []Problem{{Kind: KindEmptyOptionName}},
		},
		{
			name:       "duplicate key after trimming",
			attributes: map[string][]string{"storage": {"128 GB"}, " storage ": {"256 GB"}},
			want:       []Problem{{Field: "storage", Kind: KindDuplicateOptionName}},
		},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			_, got := NormalizeAttributes(testCase.attributes)
			if !reflect.DeepEqual(got, testCase.want) {
				t.Fatalf("problems = %#v, want %#v", got, testCase.want)
			}
		})
	}
}
