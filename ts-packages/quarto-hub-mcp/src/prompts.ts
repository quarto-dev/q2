/**
 * MCP prompts — named prompt templates clients surface as slash
 * commands (Claude Code shows this one as /mcp__quarto-hub__fix-errors).
 *
 * A prompt only instructs; the LLM does the fixing with the existing
 * tools. This is deliberately NOT a `fix_errors` tool: fixing requires
 * judgment (read the file, pick the minimal edit), which is the
 * calling agent's job — tools stay primitives.
 */

import {
  ListPromptsRequestSchema,
  GetPromptRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import type { Server } from '@modelcontextprotocol/sdk/server/index.js';

const FIX_ERRORS = {
  name: 'fix-errors',
  description:
    'Find and fix the render errors in a Quarto Hub project: checks with ' +
    'get_errors, applies minimal fixes with patch_file, and re-checks until clean.',
  arguments: [
    {
      name: 'project',
      description: "The project's automerge index document ID, or a quarto-hub.com share URL",
      required: true,
    },
    {
      name: 'path',
      description: 'Optional: fix only this file',
      required: false,
    },
  ],
};

function fixErrorsText(project: string, path?: string): string {
  const scope = path ? ` with path "${path}"` : '';
  const target = path ? `the file ${path}` : 'every affected file';
  return [
    `Fix the render errors in Quarto Hub project ${project}.`,
    '',
    `1. Call get_errors with project "${project}"${scope} to see the current errors and warnings.`,
    `2. For ${target}: read_file it, then apply the smallest fix for each error with patch_file. ` +
      'Diagnostics carry line/column, error codes, and hints — fix the reported problem and ' +
      "preserve the author's content and intent; never rewrite beyond the minimal change.",
    '3. Call get_errors again and repeat until `errors` is empty for every file you touched.',
    '4. Leave warnings alone unless they are trivially part of the same fix.',
    '5. Report each fix: file, line, what was wrong, and what you changed.',
  ].join('\n');
}

/** Register the prompt handlers on the MCP server. */
export function registerPrompts(server: Server): void {
  server.setRequestHandler(ListPromptsRequestSchema, async () => ({
    prompts: [FIX_ERRORS],
  }));

  server.setRequestHandler(GetPromptRequestSchema, async (request) => {
    const { name, arguments: args } = request.params;
    if (name !== FIX_ERRORS.name) {
      throw new Error(`Unknown prompt: ${name}`);
    }
    const project = args?.['project'];
    if (typeof project !== 'string' || project === '') {
      throw new Error("The 'project' argument is required");
    }
    const path = typeof args?.['path'] === 'string' && args['path'] !== '' ? args['path'] : undefined;
    return {
      description: FIX_ERRORS.description,
      messages: [
        {
          role: 'user' as const,
          content: { type: 'text' as const, text: fixErrorsText(project, path) },
        },
      ],
    };
  });
}
