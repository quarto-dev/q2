import XCTest
import SwiftTreeSitter
import TreeSitterDoctemplate

final class TreeSitterDoctemplateTests: XCTestCase {
    func testCanLoadGrammar() throws {
        let parser = Parser()
        let language = Language(language: tree_sitter_doctemplate())
        XCTAssertNoThrow(try parser.setLanguage(language),
                         "Error loading Doctemplate grammar")
    }
}
