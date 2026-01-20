//! Conversion between quarto-lsp-core types and tower_lsp::lsp_types.

use tower_lsp::lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticSeverity as LspSeverity,
    DocumentSymbol as LspDocumentSymbol, NumberOrString, Position as LspPosition,
    Range as LspRange, SymbolKind as LspSymbolKind,
};

use quarto_lsp_core::types::{Diagnostic, DiagnosticSeverity, Position, Range, Symbol, SymbolKind};

/// Convert a quarto-lsp-core Position to an lsp-types Position.
pub fn position_to_lsp(pos: &Position) -> LspPosition {
    LspPosition {
        line: pos.line,
        character: pos.character,
    }
}

/// Convert a quarto-lsp-core Range to an lsp-types Range.
pub fn range_to_lsp(range: &Range) -> LspRange {
    LspRange {
        start: position_to_lsp(&range.start),
        end: position_to_lsp(&range.end),
    }
}

/// Convert a quarto-lsp-core DiagnosticSeverity to an lsp-types DiagnosticSeverity.
pub fn severity_to_lsp(severity: &DiagnosticSeverity) -> LspSeverity {
    match severity {
        DiagnosticSeverity::Error => LspSeverity::ERROR,
        DiagnosticSeverity::Warning => LspSeverity::WARNING,
        DiagnosticSeverity::Information => LspSeverity::INFORMATION,
        DiagnosticSeverity::Hint => LspSeverity::HINT,
    }
}

/// Convert a quarto-lsp-core Diagnostic to an lsp-types Diagnostic.
pub fn diagnostic_to_lsp(diag: &Diagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: range_to_lsp(&diag.range),
        severity: Some(severity_to_lsp(&diag.severity)),
        code: diag.code.clone().map(NumberOrString::String),
        code_description: None,
        source: diag.source.clone(),
        message: diag.message.clone(),
        // Related information requires URIs which we don't have in the core type yet
        // For now, we skip related information. This can be enhanced in the future.
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Convert a quarto-lsp-core SymbolKind to an lsp-types SymbolKind.
pub fn symbol_kind_to_lsp(kind: &SymbolKind) -> LspSymbolKind {
    match kind {
        SymbolKind::File => LspSymbolKind::FILE,
        SymbolKind::Module => LspSymbolKind::MODULE,
        SymbolKind::Namespace => LspSymbolKind::NAMESPACE,
        SymbolKind::Package => LspSymbolKind::PACKAGE,
        SymbolKind::Class => LspSymbolKind::CLASS,
        SymbolKind::Method => LspSymbolKind::METHOD,
        SymbolKind::Property => LspSymbolKind::PROPERTY,
        SymbolKind::Field => LspSymbolKind::FIELD,
        SymbolKind::Constructor => LspSymbolKind::CONSTRUCTOR,
        SymbolKind::Enum => LspSymbolKind::ENUM,
        SymbolKind::Interface => LspSymbolKind::INTERFACE,
        SymbolKind::Function => LspSymbolKind::FUNCTION,
        SymbolKind::Variable => LspSymbolKind::VARIABLE,
        SymbolKind::Constant => LspSymbolKind::CONSTANT,
        SymbolKind::String => LspSymbolKind::STRING,
        SymbolKind::Number => LspSymbolKind::NUMBER,
        SymbolKind::Boolean => LspSymbolKind::BOOLEAN,
        SymbolKind::Array => LspSymbolKind::ARRAY,
        SymbolKind::Object => LspSymbolKind::OBJECT,
        SymbolKind::Key => LspSymbolKind::KEY,
        SymbolKind::Null => LspSymbolKind::NULL,
        SymbolKind::EnumMember => LspSymbolKind::ENUM_MEMBER,
        SymbolKind::Struct => LspSymbolKind::STRUCT,
        SymbolKind::Event => LspSymbolKind::EVENT,
        SymbolKind::Operator => LspSymbolKind::OPERATOR,
        SymbolKind::TypeParameter => LspSymbolKind::TYPE_PARAMETER,
    }
}

/// Convert a quarto-lsp-core Symbol to an lsp-types DocumentSymbol.
pub fn symbol_to_lsp(symbol: &Symbol) -> LspDocumentSymbol {
    #[allow(deprecated)]
    LspDocumentSymbol {
        name: symbol.name.clone(),
        detail: symbol.detail.clone(),
        kind: symbol_kind_to_lsp(&symbol.kind),
        tags: None,
        deprecated: None,
        range: range_to_lsp(&symbol.range),
        selection_range: range_to_lsp(&symbol.selection_range),
        children: if symbol.children.is_empty() {
            None
        } else {
            Some(symbol.children.iter().map(symbol_to_lsp).collect())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_conversion() {
        let core_pos = Position::new(10, 5);
        let lsp_pos = position_to_lsp(&core_pos);
        assert_eq!(lsp_pos.line, 10);
        assert_eq!(lsp_pos.character, 5);
    }

    #[test]
    fn test_range_conversion() {
        let core_range = Range::new(Position::new(0, 0), Position::new(0, 10));
        let lsp_range = range_to_lsp(&core_range);
        assert_eq!(lsp_range.start.line, 0);
        assert_eq!(lsp_range.start.character, 0);
        assert_eq!(lsp_range.end.line, 0);
        assert_eq!(lsp_range.end.character, 10);
    }

    #[test]
    fn test_severity_conversion() {
        assert_eq!(
            severity_to_lsp(&DiagnosticSeverity::Error),
            LspSeverity::ERROR
        );
        assert_eq!(
            severity_to_lsp(&DiagnosticSeverity::Warning),
            LspSeverity::WARNING
        );
        assert_eq!(
            severity_to_lsp(&DiagnosticSeverity::Information),
            LspSeverity::INFORMATION
        );
        assert_eq!(
            severity_to_lsp(&DiagnosticSeverity::Hint),
            LspSeverity::HINT
        );
    }

    #[test]
    fn test_diagnostic_conversion() {
        let core_diag = Diagnostic::new(
            Range::new(Position::new(0, 0), Position::new(0, 10)),
            DiagnosticSeverity::Error,
            "Test error message",
        )
        .with_code("Q-1-1");

        let lsp_diag = diagnostic_to_lsp(&core_diag);
        assert_eq!(lsp_diag.message, "Test error message");
        assert_eq!(lsp_diag.severity, Some(LspSeverity::ERROR));
        assert_eq!(lsp_diag.code, Some(NumberOrString::String("Q-1-1".into())));
    }
}
