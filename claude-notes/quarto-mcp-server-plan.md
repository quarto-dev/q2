# Quarto MCP Server - Design and Implementation Plan

## Executive Summary

This document outlines the design and implementation plan for a native MCP (Model Context Protocol) server for Quarto, accessible via `quarto mcp` command. This server will enable AI assistants like Claude to interact with Quarto projects, documents, and workflows through a standardized protocol.

## Background: Model Context Protocol (MCP)

### What is MCP?

The Model Context Protocol is an open standard (released by Anthropic in Nov 2024) for connecting AI assistants to data sources and tools. It uses JSON-RPC 2.0 over various transports (stdio, SSE, WebSocket) and provides three core primitives:

1. **Resources**: Expose data and context that AI can read
2. **Tools**: Executable functions that perform actions
3. **Prompts**: Structured templates for standardized AI interactions

### Why MCP for Quarto?

MCP is already widely adopted:
- OpenAI integrated it in March 2025 (ChatGPT, Agents SDK, Responses API)
- Microsoft Visual Studio has native support
- Multiple IDEs and AI tools support MCP servers
- Pre-built servers exist for Google Drive, Slack, GitHub, Postgres, etc.

**Key precedent**: Multiple Jupyter notebook MCP servers already exist, demonstrating the value of document-oriented MCP servers for technical/scientific workflows.

## Strategic Value for Quarto

### 1. AI-Native Document Authoring
- Enable AI assistants to understand Quarto project structure
- Allow AI to help with document creation, editing, and troubleshooting
- Provide context-aware suggestions based on project configuration

### 2. Workflow Automation
- Automate common Quarto tasks (rendering, validation, deployment)
- Enable AI-driven content generation within Quarto constraints
- Streamline multi-document project management

### 3. Enhanced Developer Experience
- Integrate with AI coding assistants (Claude, GitHub Copilot, etc.)
- Provide real-time document inspection and validation
- Enable intelligent error diagnosis and fixing

### 4. Competitive Positioning
- First technical publishing system with native MCP support
- Differentiate from Jupyter (which has 3rd-party MCP servers)
- Position Quarto as AI-forward platform

## Proposed Architecture

### Server Structure

```
quarto mcp [server-name]
  ├── stdio (default) - For Claude Desktop, Cursor, etc.
  ├── sse - For web-based clients
  └── ws - For WebSocket clients
```

### Implementation Technology

**Primary choice: Rust** (aligns with Kyoto project goals)
- Use `rmcp` or `mcp-core` Rust SDK
- Leverage existing Kyoto/quarto-markdown infrastructure
- Enable high-performance, type-safe implementation
- Natural integration with planned Rust CLI port

**Alternative: TypeScript** (faster initial prototype)
- Use existing quarto-cli codebase
- Faster time-to-market
- Later port to Rust with Kyoto

### Core Capabilities

#### A. Resources (Read-only context)

1. **Project Structure**
   - `project://config` - Project configuration (_quarto.yml)
   - `project://metadata` - Project metadata and computed config
   - `project://files` - List of project files with types
   - `project://formats` - Available output formats
   - `project://engines` - Configured computational engines

2. **Document Information**
   - `document://<path>/metadata` - YAML frontmatter
   - `document://<path>/structure` - Document outline/sections
   - `document://<path>/cells` - Code cells (for .qmd/.ipynb)
   - `document://<path>/citations` - Bibliography references
   - `document://<path>/crossrefs` - Cross-references used

3. **Schema and Validation**
   - `schema://project` - Project config schema
   - `schema://document` - Document frontmatter schema
   - `schema://format/<format>` - Format-specific options

#### B. Tools (Executable operations)

1. **Rendering Operations**
   - `render_document(path, format?, params?)` - Render single document
   - `render_project(target?, profile?)` - Render project/subset
   - `preview_document(path)` - Start live preview

2. **Inspection & Analysis**
   - `inspect_project(path?)` - Get project structure (wraps `quarto inspect`)
   - `inspect_document(path)` - Get document metadata and dependencies
   - `validate_yaml(path)` - Validate YAML against schema
   - `list_formats(path)` - Available formats for document

3. **Content Operations**
   - `execute_cell(path, cell_id)` - Execute specific code cell
   - `list_cells(path)` - Get all code cells from document
   - `get_cell_output(path, cell_id)` - Retrieve cell execution results

4. **Project Management**
   - `create_document(path, template?, engine?)` - Create new document
   - `add_format(path, format, options?)` - Add output format
   - `check_dependencies()` - Check required tools/packages

5. **Publishing**
   - `publish_document(path, destination)` - Publish to Quarto Pub, GitHub Pages, etc.
   - `list_publish_targets()` - Available publishing destinations

#### C. Prompts (Workflow templates)

1. **Document Creation**
   - `create_article` - Interactive article creation workflow
   - `create_presentation` - Presentation creation with format selection
   - `create_notebook` - Computational notebook setup

2. **Troubleshooting**
   - `debug_render_error` - Analyze and suggest fixes for render errors
   - `fix_yaml` - Interactive YAML error fixing
   - `check_setup` - Validate Quarto environment

3. **Content Generation**
   - `add_citation` - Add formatted citation
   - `create_figure` - Generate figure with proper crossref
   - `add_table` - Create formatted table

4. **Project Setup**
   - `init_website` - Initialize website project
   - `init_book` - Initialize book project
   - `configure_publishing` - Set up publishing workflow

## Implementation Plan

### Phase 1: Core Infrastructure (2-3 weeks)

**Goal**: Basic MCP server with essential resources

1. **Week 1: Setup & Scaffolding**
   - Set up Rust project with rmcp/mcp-core
   - Implement basic server with stdio transport
   - Create `quarto mcp` CLI command integration
   - Design data structure for resources/tools

2. **Week 2-3: Core Resources**
   - Implement project config resource (`project://config`)
   - Implement document metadata resource (`document://<path>/metadata`)
   - Add project file listing (`project://files`)
   - Basic error handling and logging

**Deliverable**: `quarto mcp` server that exposes basic project/document info

### Phase 2: Essential Tools (3-4 weeks)

**Goal**: Key operational tools for rendering and inspection

1. **Week 4-5: Inspection Tools**
   - `inspect_project()` - Wrap existing `quarto inspect`
   - `inspect_document()` - Document-level inspection
   - `validate_yaml()` - YAML validation with error reporting

2. **Week 6-7: Rendering Tools**
   - `render_document()` - Single document rendering
   - `render_project()` - Project rendering
   - Capture and return rendering output/errors

**Deliverable**: AI can inspect and render Quarto projects

### Phase 3: Advanced Features (3-4 weeks)

**Goal**: Code execution, content operations, prompts

1. **Week 8-9: Code Cell Operations**
   - `list_cells()` - Extract code cells
   - `execute_cell()` - Execute individual cells
   - `get_cell_output()` - Return execution results
   - Support for Jupyter, Knitr engines

2. **Week 10-11: Prompts & Workflows**
   - Implement core prompts (document creation, debugging)
   - Add content generation helpers
   - Create troubleshooting workflows

**Deliverable**: Full-featured MCP server with AI-friendly workflows

### Phase 4: Production & Integration (2 weeks)

**Goal**: Production-ready, documented, integrated

1. **Week 12-13: Polish & Documentation**
   - Comprehensive testing
   - Error handling refinement
   - Usage documentation and examples
   - Integration guides for Claude Desktop, Cursor, VS Code
   - Performance optimization

**Deliverable**: Production-ready `quarto mcp` command

## Technical Considerations

### Integration with Kyoto (Rust Port)

The MCP server aligns perfectly with Kyoto goals:

1. **Shared Infrastructure**
   - Reuse quarto-markdown parser for document analysis
   - Share YAML validation system (when ported)
   - Leverage unified SourceInfo for error reporting

2. **Incremental Development**
   - MCP server can start simple, grow with Kyoto
   - Resources can be added as Kyoto capabilities mature
   - Natural testing ground for Rust implementations

3. **LSP Synergy**
   - MCP and LSP share similar architecture (both use JSON-RPC)
   - Can share document parsing/analysis code
   - MCP provides AI interface, LSP provides IDE interface

### Security & Sandboxing

Important considerations:

1. **Code Execution**
   - Cell execution must respect Quarto's existing security model
   - Consider sandboxing for untrusted code
   - Configurable execution permissions

2. **File System Access**
   - Limit to project directory by default
   - Explicit opt-in for broader file access
   - Validate all paths to prevent traversal

3. **Publishing Operations**
   - Require explicit user confirmation for publishing
   - Validate credentials and destinations
   - Audit log for automated operations

### Transport Options

**Primary: stdio** (simplest, most compatible)
- Direct integration with Claude Desktop, Cursor
- No network configuration required
- Process-to-process communication

**Optional: SSE/WebSocket**
- For web-based clients
- Enables remote access scenarios
- Requires authentication/authorization

## Competitive Analysis

### Existing Solutions

1. **Jupyter MCP Servers** (3+ implementations)
   - Focus on notebook execution
   - Limited project-level features
   - No publishing integration
   - Quarto can differentiate with multi-format, project-aware features

2. **Generic Markdown Tools**
   - Basic markdown editing/rendering
   - No computational features
   - No publishing workflows
   - Quarto's scientific/technical focus is unique

### Quarto Advantages

1. **Unified Platform**: Single MCP server for documents, notebooks, websites, books
2. **Multi-Engine**: Support for Jupyter, Knitr, Observable in one interface
3. **Publishing**: Direct integration with Quarto Pub and other platforms
4. **Schema-Driven**: Rich validation and intelligent assistance
5. **Project-Aware**: Understand websites/books, not just individual files

## Success Metrics

### Technical Metrics
- MCP server response time < 200ms for resource queries
- Support for 3+ MCP clients (Claude Desktop, Cursor, VS Code)
- Handle projects with 100+ documents efficiently
- 95%+ success rate for render operations

### Adoption Metrics
- Integration examples for major AI coding assistants
- Community MCP server extensions
- Usage in Quarto documentation/examples
- Positive feedback from early adopters

### Feature Completeness
- 10+ resources exposed
- 15+ tools implemented
- 5+ prompt templates
- Full test coverage (>80%)

## Risks & Mitigation

### Risk 1: Premature Specification
**Risk**: MCP protocol evolves, breaking compatibility
**Mitigation**:
- Track MCP spec changes actively
- Version MCP server capabilities
- Maintain backward compatibility layer

### Risk 2: Performance with Large Projects
**Risk**: Slow response times for large websites/books
**Mitigation**:
- Implement caching for project metadata
- Lazy loading for resource-intensive operations
- Parallel rendering where possible

### Risk 3: Maintenance Burden
**Risk**: MCP server becomes complex to maintain
**Mitigation**:
- Share code with core Quarto CLI
- Comprehensive testing from day 1
- Clear separation of concerns
- Good documentation

### Risk 4: Limited Adoption
**Risk**: Users don't discover/use MCP integration
**Mitigation**:
- Prominent documentation
- Integration guides for popular tools
- Example workflows and use cases
- Community engagement

## Next Steps

### Immediate Actions (This Week)

1. **Prototype Decision**: Choose Rust vs TypeScript
   - Consider: Speed to prototype vs long-term alignment
   - Recommendation: **Start with Rust** (aligns with Kyoto, forces good architecture)

2. **Spike Implementation**: 2-day spike
   - Basic MCP server with 1 resource, 1 tool
   - Validate rmcp/mcp-core suitability
   - Test integration with Claude Desktop

3. **Detailed Design**: Finalize resource/tool schemas
   - JSON schema for all resources
   - Tool parameter definitions
   - Error response formats

### Week 2-3: Foundation

1. Implement core MCP server infrastructure
2. Add `quarto mcp` CLI command
3. Implement first 3-5 resources
4. Document architecture decisions

### Month 2: Tools & Features

1. Implement inspection and rendering tools
2. Add code execution capabilities
3. Create initial prompt templates
4. Begin testing with real projects

### Month 3: Production

1. Complete remaining tools/resources
2. Security audit and hardening
3. Performance optimization
4. Documentation and examples
5. Beta release

## Conclusion

A native Quarto MCP server represents a significant opportunity to position Quarto as the AI-native platform for technical publishing. The implementation aligns perfectly with the Kyoto Rust port initiative, providing both immediate value and a foundation for future AI integrations.

**Key Advantages**:
- ✅ First-mover advantage in technical publishing space
- ✅ Natural fit with Quarto's multi-format, project-oriented model
- ✅ Synergy with ongoing Rust port (Kyoto)
- ✅ Clear differentiation from Jupyter and generic markdown tools
- ✅ Growing ecosystem (OpenAI, Microsoft, Anthropic support)

**Recommended Path**:
1. Start 2-day Rust spike to validate approach
2. Proceed with Phase 1 (core infrastructure) if successful
3. Iterate based on user feedback
4. Integrate deeply with Kyoto as it matures

The `quarto mcp` command could become a flagship feature, demonstrating Quarto's commitment to modern, AI-assisted workflows while maintaining the quality and rigor expected in scientific and technical publishing.
