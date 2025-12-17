/*
 * debug_comrak.rs
 * Debug test to understand comrak's AST structure for whitespace handling.
 */

fn main() {}

#[cfg(test)]
mod tests {
    use comrak::{parse_document, Arena, Options};
    use comrak::nodes::NodeValue;

    #[test]
    fn debug_comrak_html_output() {
        let markdown = "aA *aA*\n";

        // Test HTML output
        let html_str = comrak::markdown_to_html(markdown, &Options::default());

        eprintln!("=== MARKDOWN ===");
        eprintln!("{:?}", markdown);
        eprintln!("\n=== HTML OUTPUT ===");
        eprintln!("{:?}", html_str);
    }

    #[test]
    fn debug_comrak_ast_text_nodes() {
        let markdown = "aA *aA*\n";

        let arena = Arena::new();
        let options = Options::default();
        let root = parse_document(&arena, markdown, &options);

        eprintln!("=== MARKDOWN ===");
        eprintln!("{:?}", markdown);

        eprintln!("\n=== RAW AST TREE ===");
        fn print_node<'a>(node: &'a comrak::arena_tree::Node<'a, std::cell::RefCell<comrak::nodes::Ast>>, indent: usize) {
            let data = node.data.borrow();
            let indent_str = "  ".repeat(indent);

            match &data.value {
                NodeValue::Text(text) => {
                    eprintln!("{}Text({:?}) [bytes: {:?}]", indent_str, text, text.as_bytes());
                }
                other => {
                    eprintln!("{}{:?}", indent_str, other);
                }
            }

            for child in node.children() {
                print_node(child, indent + 1);
            }
        }

        print_node(root, 0);
    }

    #[test]
    fn debug_comrak_ast_with_spaces() {
        // Test various space scenarios
        let test_cases = [
            "aA *aA*\n",
            "hello *world*\n",
            "a *b* c\n",
            "*a* b\n",
            "a*b*\n",  // No space before emphasis
            "`code1` `code2`\n",  // Code spans with space
        ];

        for markdown in test_cases {
            eprintln!("\n========================================");
            eprintln!("INPUT: {:?}", markdown);
            eprintln!("========================================");

            let arena = Arena::new();
            let options = Options::default();
            let root = parse_document(&arena, markdown, &options);

            // Get the paragraph's children
            if let Some(para) = root.first_child() {
                eprintln!("Children of paragraph:");
                for child in para.children() {
                    let data = child.data.borrow();
                    match &data.value {
                        NodeValue::Text(text) => {
                            eprintln!("  Text({:?}) bytes={:?}", text, text.as_bytes());
                        }
                        NodeValue::Emph => {
                            eprintln!("  Emph:");
                            for emph_child in child.children() {
                                let emph_data = emph_child.data.borrow();
                                match &emph_data.value {
                                    NodeValue::Text(text) => {
                                        eprintln!("    Text({:?})", text);
                                    }
                                    other => {
                                        eprintln!("    {:?}", other);
                                    }
                                }
                            }
                        }
                        NodeValue::Code(code) => {
                            eprintln!("  Code({:?})", code.literal);
                        }
                        other => {
                            eprintln!("  {:?}", other);
                        }
                    }
                }
            }

            // Also show HTML output
            let html = comrak::markdown_to_html(markdown, &Options::default());
            eprintln!("HTML: {:?}", html);
        }
    }
}
