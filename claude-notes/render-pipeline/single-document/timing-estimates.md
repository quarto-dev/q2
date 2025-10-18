## Timing Estimates

Based on code structure and typical execution:

| Stage | Time (typical) | Time (complex) | Notes |
|-------|----------------|----------------|-------|
| 1. CLI Entry | <10ms | <10ms | Argument parsing |
| 2. Render Coordinator | <50ms | <100ms | Project detection, context setup |
| 3. File Rendering Setup | <10ms | <10ms | Temp context |
| 4. Context Creation | 50-200ms | 200-500ms | Format resolution, metadata merging |
| 5. Engine Selection | <50ms | <50ms | File inspection |
| 6. YAML Validation | 50-100ms | 100-200ms | Schema loading, validation |
| 7. Engine Execution | **100ms-10s+** | **1s-60s+** | Depends on computation |
| 8. Language Handlers | 50-500ms | 500ms-5s | OJS compilation, diagrams |
| 9. Pandoc | **500ms-5s** | **5s-60s** | Filters, conversion |
| 10. Postprocessing | 100-500ms | 500ms-2s | HTML manipulation, cleanup |
| **Total** | **1-20s** | **10-120s** | Highly variable |

**Bottlenecks:**
- Engine execution (stage 7) - dominates for computational documents
- Pandoc conversion (stage 9) - significant for large/complex documents
- Everything else is < 1s typically

