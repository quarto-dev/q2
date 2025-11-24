package tree_sitter_doctemplate_test

import (
	"testing"

	tree_sitter "github.com/tree-sitter/go-tree-sitter"
	tree_sitter_doctemplate "github.com/tree-sitter/tree-sitter-doctemplate/bindings/go"
)

func TestCanLoadGrammar(t *testing.T) {
	language := tree_sitter.NewLanguage(tree_sitter_doctemplate.Language())
	if language == nil {
		t.Errorf("Error loading Doctemplate grammar")
	}
}
