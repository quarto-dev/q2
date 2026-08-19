    fn temp_investigation_cite_citations_updated_when_changed() {
        use quarto_pandoc_types::{Citation, CitationMode, Cite};
        fn cite_para(
            id: &str,
            prefix_text: &str,
            source: SourceInfo,
        ) -> quarto_pandoc_types::Block {
            quarto_pandoc_types::Block::Paragraph(Paragraph {
                content: vec![
                    quarto_pandoc_types::Inline::Str(Str {
                        text: "anchor".to_string(),
                        source_info: source.clone(),
                    }),
                    quarto_pandoc_types::Inline::Cite(Cite {
                        citations: vec![Citation {
                            id: id.to_string(),
                            prefix: vec![quarto_pandoc_types::Inline::Str(Str {
                                text: prefix_text.to_string(),
                                source_info: source.clone(),
                            })],
                            suffix: vec![],
                            mode: CitationMode::NormalCitation,
                            note_num: 0,
                            hash: 0,
                            id_source: None,
                        }],
                        content: vec![quarto_pandoc_types::Inline::Str(Str {
                            text: "shared".to_string(),
                            source_info: source.clone(),
                        })],
                        source_info: source.clone(),
                    }),
                ],
                source_info: source,
            })
        }
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![cite_para("a", "x", source_original())],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![cite_para("b", "y", source_executed())],
        };
        let executed_clone = executed.clone();
        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original, executed, &plan);
        assert!(
            crate::hash::structural_eq_blocks(&result.blocks, &executed_clone.blocks),
            "Result: {:?}\nAfter: {:?}",
            result.blocks,
            executed_clone.blocks
        );
    }
