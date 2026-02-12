/**
 * Template Service
 *
 * Discovers and processes project templates from _quarto-hub-templates directory.
 * Templates are .qmd files with optional template-name metadata for display names.
 */

import { vfsReadFile, vfsListFiles } from './wasmRenderer';

// Use dynamic import to avoid requiring WASM at module load time
let prepareTemplateFunc: ((content: string) => string) | null = null;

async function getPrepareTemplate(): Promise<(content: string) => string> {
  if (!prepareTemplateFunc) {
    const wasm = await import('wasm-quarto-hub-client');
    prepareTemplateFunc = wasm.prepare_template;
  }
  return prepareTemplateFunc;
}

/**
 * A project template that can be used to create new files.
 */
export interface ProjectTemplate {
  /** Full VFS path, e.g., "/project/_quarto-hub-templates/article.qmd" */
  path: string;
  /** Display name (from template-name metadata or filename) */
  displayName: string;
  /** Template content with template-name metadata removed */
  strippedContent: string;
}

/** Directory where templates are stored (with /project/ prefix for VFS) */
const TEMPLATES_DIR = '/project/_quarto-hub-templates/';

/**
 * Discover all templates in the project.
 *
 * Scans the VFS for .qmd files in _quarto-hub-templates/ and processes them
 * to extract template names and prepare content for use.
 *
 * @returns Array of discovered templates, sorted alphabetically by display name
 */
export async function discoverTemplates(): Promise<ProjectTemplate[]> {
  const prepareTemplate = await getPrepareTemplate();

  // Get all files from VFS
  const listResult = vfsListFiles();
  if (!listResult.success || !listResult.files) {
    console.warn('[templateService] Failed to list VFS files');
    return [];
  }

  // Filter to template files (top-level .qmd files in _quarto-hub-templates/)
  const templateFiles = listResult.files.filter((f) => {
    if (!f.startsWith(TEMPLATES_DIR)) return false;
    if (!f.endsWith('.qmd')) return false;
    // Only top-level files (no additional path separators after the templates dir)
    const relativePath = f.slice(TEMPLATES_DIR.length);
    return !relativePath.includes('/');
  });

  const templates: ProjectTemplate[] = [];

  for (const path of templateFiles) {
    try {
      // Read the template content
      const readResult = vfsReadFile(path);
      if (!readResult.success || !readResult.content) {
        console.warn(`[templateService] Failed to read template: ${path}`);
        continue;
      }

      // Process the template to extract name and strip metadata
      const result = JSON.parse(prepareTemplate(readResult.content));
      if (!result.success) {
        console.warn(`[templateService] Failed to process template ${path}: ${result.error}`);
        continue;
      }

      // Use template-name if present, otherwise derive from filename
      const filename = path.slice(TEMPLATES_DIR.length);
      const displayName = result.template_name ?? filename.replace(/\.qmd$/, '');

      templates.push({
        path,
        displayName,
        strippedContent: result.stripped_content,
      });
    } catch (e) {
      console.warn(`[templateService] Error processing template ${path}:`, e);
    }
  }

  // Sort alphabetically by display name
  templates.sort((a, b) => a.displayName.localeCompare(b.displayName));

  return templates;
}

/**
 * Check if a project has any templates available.
 *
 * This is a quick check that doesn't process the templates, useful for
 * deciding whether to show the template selector in the UI.
 */
export function hasTemplates(): boolean {
  const listResult = vfsListFiles();
  if (!listResult.success || !listResult.files) {
    return false;
  }

  return listResult.files.some((f) => {
    if (!f.startsWith(TEMPLATES_DIR)) return false;
    if (!f.endsWith('.qmd')) return false;
    const relativePath = f.slice(TEMPLATES_DIR.length);
    return !relativePath.includes('/');
  });
}
