# Schemas for validating writer output

`ooxml/` holds the ECMA-376 **transitional** XML schemas (the
`schemas.openxmlformats.org` namespaces Word writes), taken from python-docx
`ref/xsd/` at v1.2.0 (`e454546`, MIT; `LICENSE-python-docx`), plus W3C's
`xml.xsd` (W3C Software License, notice inside the file). It is the transitive
import closure of `shared-math.xsd` (OMML): `wml.xsd` (WordprocessingML),
`shared-commonSimpleTypes.xsd`, `shared-relationshipReference.xsd`,
`shared-customXmlSchemaProperties.xsd` and the DrawingML schemas `wml.xsd`
pulls in. Twelve files plus `xml.xsd`, about 500 KB.

**One local patch, in the two files that import the `xml:` namespace
(`shared-math.xsd`, `wml.xsd`):** that import gained
`schemaLocation="xml.xsd"`. Upstream omits it, and libxml2 (`xmllint`) then
cannot resolve `xml:space`, so the schema set fails to compile. This is the
same patch pandoc's docx validator carries.

Used by:

- `quarto-math`'s OMML writer tests (`tests/integration/omml_schema.rs`):
  each emitted `m:oMath` is validated with `xmllint --schema shared-math.xsd`.
  The helper skips, loudly, when `xmllint` is not on `PATH` (Windows CI).
- Phase 2 of the native docx writer will validate whole `document.xml` parts
  against `wml.xsd` from the same directory; the closure is here so that step
  needs no second copy. Word-saved parts fail a plain XSD pass on
  `mc:Ignorable` (markup compatibility) and need an MCE strip first; our own
  writer output does not emit MCE.

Verified 2026-09-21 with libxml 2.9.13: a correct `m:oMath` (fraction,
superscript, n-ary with limits) validates; `m:den` before `m:num` is rejected
naming the expected elements; a minimal `w:document` containing `m:oMath`
validates against `wml.xsd` in about 25 ms.
