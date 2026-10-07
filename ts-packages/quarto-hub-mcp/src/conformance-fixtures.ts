/**
 * The `test_*` fixture surface the official MCP conformance suite
 * (`@modelcontextprotocol/conformance`) is written against (Phase 6,
 * bd-8iv9jty5). Each scenario names the tool/prompt/resource it drives
 * (`test_simple_text`, `test://static-text`, …) — the surface is how
 * the suite exercises OUR server construction, validation, and
 * serialization rather than a reference side-build.
 *
 * Registered only when `createServer` is called with
 * `conformanceFixtures: true` (the Phase 6 loopback listener; never in
 * production): the default tool listing and the ERG-5 budget are
 * untouched, and no fixture name can collide with a real tool.
 *
 * MRTR (SEP-2322 `input_required`) fixtures use the SDK builders
 * (`inputRequired`, `acceptedContent`, `inputResponse`) and the HMAC
 * requestState codec — under one-server-per-request serving each round
 * hits a fresh instance, so the codec key is a module constant shared
 * by every construction. The key protects integrity only; it is not a
 * secret and the payload stays client-readable by design.
 */

import { z } from 'zod';
import {
  acceptedContent,
  CLIENT_CAPABILITIES_META_KEY,
  createRequestStateCodec,
  inputRequired,
  inputResponse,
  LOG_LEVEL_META_KEY,
  ResourceNotFoundError,
  type McpServer,
} from '@modelcontextprotocol/server';

/** Shared HMAC key for the fixture requestState codec (≥ 32 bytes).
 * Test-only: integrity, not confidentiality — see module docstring. */
const FIXTURE_STATE_KEY =
  'quarto-hub-mcp conformance fixture state key (not a secret)';

/** The codec every fixture instance mints/verifies with — must be one
 * constant across instances because per-request serving spreads the
 * rounds of a flow across fresh servers. */
export const conformanceRequestState = createRequestStateCodec<Record<string, unknown>>({
  key: FIXTURE_STATE_KEY,
});

/** 1×1 red-pixel PNG. */
const PNG_BASE64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';

/** Minimal 44-byte-header WAV (PCM, 8 kHz mono, zero samples). */
const WAV_BASE64 = 'UklGRiQAAABXQVZFZm10IBAAAAABAAEAIlYAAESsAAACABAAZGF0YQAAAAA=';

const EMBEDDED_RESOURCE_TEXT = 'This is an embedded resource content.';
const STATIC_TEXT = 'This is the content of the static text resource.';

/** Fixture resources served by `resources/read` when enabled. The map
 * lives here (not inline in resources.ts) so the read side and the
 * list side can never drift. */
export const FIXTURE_RESOURCES: ReadonlyArray<{
  uri: string;
  name: string;
  description: string;
  mimeType: string;
  text?: string;
  blob?: string;
}> = [
  {
    uri: 'test://static-text',
    name: 'static-text',
    description: 'A static text resource for conformance testing.',
    mimeType: 'text/plain',
    text: STATIC_TEXT,
  },
  {
    uri: 'test://static-binary',
    name: 'static-binary',
    description: 'A static binary resource for conformance testing.',
    mimeType: 'image/png',
    blob: PNG_BASE64,
  },
];

/** The fixture resource template (`resources/templates/list` entry). */
export const FIXTURE_RESOURCE_TEMPLATE = {
  name: 'test-template',
  title: 'Conformance test template',
  uriTemplate: 'test://template/{id}/data',
  description: 'Echoes the {id} path parameter as text content.',
  mimeType: 'text/plain',
};

/**
 * Read a `test://` fixture URI. Returns the contents array for a known
 * fixture (statics and the template), `null` for non-`test://` URIs
 * (caller falls through to hub:// handling), and throws
 * `ResourceNotFoundError` for unknown `test://` URIs (the
 * sep-2164-resource-not-found scenario).
 */
export function readFixtureResource(
  uri: string,
): Array<
  | { uri: string; mimeType: string; text: string }
  | { uri: string; mimeType: string; blob: string }
> | null {
  if (!uri.startsWith('test://')) return null;
  const staticHit = FIXTURE_RESOURCES.find((r) => r.uri === uri);
  if (staticHit) {
    if (staticHit.text !== undefined) {
      return [{ uri: staticHit.uri, mimeType: staticHit.mimeType, text: staticHit.text }];
    }
    return [{ uri: staticHit.uri, mimeType: staticHit.mimeType, blob: staticHit.blob! }];
  }
  const templateMatch = /^test:\/\/template\/([^/]+)\/data$/.exec(uri);
  if (templateMatch) {
    return [
      {
        uri,
        mimeType: 'text/plain',
        text: `Template data for id: ${decodeURIComponent(templateMatch[1]!)}`,
      },
    ];
  }
  throw new ResourceNotFoundError(uri);
}

const nameSchema = z.object({ name: z.string() });
const USER_NAME_REQUEST = () =>
  inputRequired.elicit({
    message: 'What is your name?',
    requestedSchema: {
      type: 'object',
      properties: { name: { type: 'string' } },
      required: ['name'],
    },
  });

const contextSchema = z.object({ context: z.string() });
const USER_CONTEXT_REQUEST = () =>
  inputRequired.elicit({
    message: 'What context should the prompt use?',
    requestedSchema: {
      type: 'object',
      properties: { context: { type: 'string' } },
      required: ['context'],
    },
  });

const text = (t: string) => ({ content: [{ type: 'text' as const, text: t }] });

/**
 * Register the fixture tools, prompts, and the completion surface.
 * Resources are wired inside `registerResources` (its read/list handlers
 * own the URI dispatch — see `readFixtureResource`).
 */
export function registerConformanceFixtures(server: McpServer): void {
  // -- simple content-shape tools -------------------------------------
  server.registerTool(
    'test_simple_text',
    { description: 'Returns a simple text response (conformance fixture).', inputSchema: z.object({}) },
    () => text('This is a simple text response for testing.'),
  );
  server.registerTool(
    'test_image_content',
    { description: 'Returns image content (conformance fixture).', inputSchema: z.object({}) },
    () => ({ content: [{ type: 'image' as const, data: PNG_BASE64, mimeType: 'image/png' }] }),
  );
  server.registerTool(
    'test_audio_content',
    { description: 'Returns audio content (conformance fixture).', inputSchema: z.object({}) },
    () => ({ content: [{ type: 'audio' as const, data: WAV_BASE64, mimeType: 'audio/wav' }] }),
  );
  server.registerTool(
    'test_embedded_resource',
    {
      description: 'Returns embedded resource content (conformance fixture).',
      inputSchema: z.object({}),
    },
    () => ({
      content: [
        {
          type: 'resource' as const,
          resource: {
            uri: 'test://embedded-resource',
            mimeType: 'text/plain',
            text: EMBEDDED_RESOURCE_TEXT,
          },
        },
      ],
    }),
  );
  server.registerTool(
    'test_multiple_content_types',
    {
      description: 'Returns mixed text/image/resource content (conformance fixture).',
      inputSchema: z.object({}),
    },
    () => ({
      content: [
        { type: 'text' as const, text: 'Multiple content types test:' },
        { type: 'image' as const, data: PNG_BASE64, mimeType: 'image/png' },
        {
          type: 'resource' as const,
          resource: {
            uri: 'test://mixed-content-resource',
            mimeType: 'text/plain',
            text: EMBEDDED_RESOURCE_TEXT,
          },
        },
      ],
    }),
  );
  server.registerTool(
    'test_error_handling',
    {
      description: 'Always returns an error result (conformance fixture).',
      inputSchema: z.object({}),
    },
    () => ({
      isError: true,
      content: [{ type: 'text' as const, text: 'This tool intentionally returns an error for testing' }],
    }),
  );
  server.registerTool(
    'test_tool_with_progress',
    {
      description: 'Emits 0/50/100 progress notifications (conformance fixture).',
      inputSchema: z.object({}),
    },
    async (_args, ctx) => {
      const progressToken = ctx?.mcpReq?._meta?.progressToken;
      if (progressToken !== undefined) {
        for (const progress of [0, 50, 100]) {
          await ctx.mcpReq.notify({
            method: 'notifications/progress',
            params: { progressToken, progress, total: 100 },
          });
          if (progress < 100) await new Promise((r) => setTimeout(r, 50));
        }
      } else {
        await new Promise((r) => setTimeout(r, 150));
      }
      return text('Progress notifications test completed successfully.');
    },
  );

  // -- MRTR (SEP-2322 input_required) tools ---------------------------
  server.registerTool(
    'test_input_required_result_elicitation',
    {
      description: 'Round-trips one elicitation input request (conformance fixture).',
      inputSchema: z.object({}),
    },
    (_args, ctx) => {
      const answered = acceptedContent(ctx.mcpReq.inputResponses, 'user_name', nameSchema);
      if (answered !== undefined) {
        return text(`Elicitation input received and accepted (name: ${answered.name}).`);
      }
      // Missing, declined, or malformed: re-issue the request — never a
      // hard error (missing-input-response / validate-input scenarios).
      return inputRequired({ inputRequests: { user_name: USER_NAME_REQUEST() } });
    },
  );
  server.registerTool(
    'test_input_required_result_sampling',
    {
      description: 'Round-trips one sampling input request (conformance fixture).',
      inputSchema: z.object({}),
    },
    (_args, ctx) => {
      const view = inputResponse(ctx.mcpReq.inputResponses, 'sample');
      if (view.kind === 'sampling') {
        const content = view.result.content;
        const blocks = Array.isArray(content) ? content : [content];
        const textBlock = blocks.find((b) => b?.type === 'text');
        const got = textBlock?.type === 'text' ? textBlock.text : '(non-text content)';
        return text(`Sampling input received: ${got}`);
      }
      return inputRequired({
        inputRequests: {
          sample: inputRequired.createMessage({
            messages: [
              { role: 'user', content: { type: 'text', text: 'Reply with any short text.' } },
            ],
            maxTokens: 50,
          }),
        },
      });
    },
  );
  server.registerTool(
    'test_input_required_result_list_roots',
    {
      description: 'Round-trips one roots-listing input request (conformance fixture).',
      inputSchema: z.object({}),
    },
    (_args, ctx) => {
      const view = inputResponse(ctx.mcpReq.inputResponses, 'roots');
      if (view.kind === 'roots') {
        return text(`Roots input received: ${view.roots.length} root(s).`);
      }
      return inputRequired({ inputRequests: { roots: inputRequired.listRoots() } });
    },
  );
  server.registerTool(
    'test_input_required_result_request_state',
    {
      description: 'Round-trips integrity-protected requestState (conformance fixture).',
      inputSchema: z.object({}),
    },
    async (_args, ctx) => {
      const state = ctx.mcpReq.requestState<Record<string, unknown>>();
      if (state !== undefined) {
        return text(`requestState verified and round-tripped: ${JSON.stringify(state)}`);
      }
      return inputRequired({
        inputRequests: { user_name: USER_NAME_REQUEST() },
        requestState: await conformanceRequestState.mint({ round: 1 }),
      });
    },
  );
  server.registerTool(
    'test_input_required_result_multiple_inputs',
    {
      description:
        'Requires elicitation + sampling + roots inputs and requestState (conformance fixture).',
      inputSchema: z.object({}),
    },
    async (_args, ctx) => {
      const name = acceptedContent(ctx.mcpReq.inputResponses, 'user_name', nameSchema);
      const sample = inputResponse(ctx.mcpReq.inputResponses, 'sample');
      const roots = inputResponse(ctx.mcpReq.inputResponses, 'client_roots');
      const state = ctx.mcpReq.requestState<Record<string, unknown>>();
      if (
        name !== undefined &&
        sample.kind === 'sampling' &&
        roots.kind === 'roots' &&
        state !== undefined
      ) {
        return text(
          `All inputs received: name=${name.name}, roots=${roots.roots.length}, state verified.`,
        );
      }
      return inputRequired({
        inputRequests: {
          user_name: USER_NAME_REQUEST(),
          sample: inputRequired.createMessage({
            messages: [
              { role: 'user', content: { type: 'text', text: 'Reply with any short text.' } },
            ],
            maxTokens: 50,
          }),
          client_roots: inputRequired.listRoots(),
        },
        requestState: await conformanceRequestState.mint({ round: 1 }),
      });
    },
  );
  server.registerTool(
    'test_input_required_result_multi_round',
    {
      description: 'A three-round input flow driven by requestState (conformance fixture).',
      inputSchema: z.object({}),
    },
    async (_args, ctx) => {
      const state = ctx.mcpReq.requestState<{ round?: number }>();
      if (state?.round === 2) {
        return text('Multi-round flow completed.');
      }
      if (state?.round === 1) {
        return inputRequired({
          inputRequests: {
            second: inputRequired.elicit({
              message: 'Second round: confirm again',
              requestedSchema: {
                type: 'object',
                properties: { ok: { type: 'boolean' } },
                required: ['ok'],
              },
            }),
          },
          requestState: await conformanceRequestState.mint({ round: 2 }),
        });
      }
      return inputRequired({
        inputRequests: { first: USER_NAME_REQUEST() },
        requestState: await conformanceRequestState.mint({ round: 1 }),
      });
    },
  );
  server.registerTool(
    'test_input_required_result_tampered_state',
    {
      description:
        'Mints signed requestState; the server seam rejects tampering (conformance fixture).',
      inputSchema: z.object({}),
    },
    async (_args, ctx) => {
      const state = ctx.mcpReq.requestState<Record<string, unknown>>();
      if (state !== undefined) {
        return text('Signed state verified.');
      }
      return inputRequired({
        inputRequests: { user_name: USER_NAME_REQUEST() },
        requestState: await conformanceRequestState.mint({ marker: 'fixture-state' }),
      });
    },
  );
  server.registerTool(
    'test_input_required_result_capabilities',
    {
      description:
        'Includes only input requests for capabilities the client declared (conformance fixture).',
      inputSchema: z.object({}),
    },
    (_args, ctx) => {
      // The capability contract (SEP-2322): only ever ask for what the
      // client declared in the per-request capabilities envelope (the
      // reserved io.modelcontextprotocol/* keys are lifted out of
      // `_meta` into `envelope` — reading `_meta` finds nothing). The
      // SDK seam also refuses undeclared kinds — but with a wire error,
      // where the scenario expects a well-formed input_required.
      // The envelope is typed as an opaque bag; the SDK keys it by the
      // full meta-key strings (its own read site does the same cast).
      const envelope = ctx.mcpReq.envelope as Record<string, unknown> | undefined;
      const declared =
        (envelope?.[CLIENT_CAPABILITIES_META_KEY] as
          | { sampling?: unknown; elicitation?: unknown; roots?: unknown }
          | undefined) ?? {};
      const inputRequests: Record<string, ReturnType<typeof USER_NAME_REQUEST>> = {};
      if (declared.sampling !== undefined) {
        inputRequests['sample'] = inputRequired.createMessage({
          messages: [
            { role: 'user', content: { type: 'text', text: 'Reply with any short text.' } },
          ],
          maxTokens: 50,
        });
      }
      if (declared.elicitation !== undefined) {
        inputRequests['user_name'] = USER_NAME_REQUEST();
      }
      if (declared.roots !== undefined) {
        inputRequests['client_roots'] = inputRequired.listRoots();
      }
      if (Object.keys(inputRequests).length > 0) {
        return inputRequired({ inputRequests });
      }
      return text('Client declares no input capabilities; nothing to request.');
    },
  );
  server.registerTool(
    'test_streaming_elicitation',
    {
      description:
        'An elicitation flow over a streamed response; the stream must carry no ' +
        'independent server-to-client requests (SEP-2575 diagnostic fixture).',
      inputSchema: z.object({}),
    },
    (_args, ctx) => {
      const answered = acceptedContent(ctx.mcpReq.inputResponses, 'user_name', nameSchema);
      if (answered !== undefined) {
        return text(`Streaming elicitation completed (name: ${answered.name}).`);
      }
      // The 2026-07-28 stateless model routes input requests through the
      // input_required result — never as independent stream frames.
      return inputRequired({ inputRequests: { user_name: USER_NAME_REQUEST() } });
    },
  );
  server.registerTool(
    'test_logging_tool',
    {
      description:
        'Emits a log notification only when the request set a logLevel ' +
        '(SEP-2575 no-log-without-logLevel diagnostic fixture).',
      inputSchema: z.object({}),
    },
    async (_args, ctx) => {
      const envelope = ctx.mcpReq.envelope as Record<string, unknown> | undefined;
      const logLevel = envelope?.[LOG_LEVEL_META_KEY];
      if (logLevel !== undefined) {
        await ctx.mcpReq.notify({
          method: 'notifications/message',
          params: { level: 'info', logger: 'conformance-fixture', data: 'logLevel was set' },
        });
      }
      return text('Logging fixture executed.');
    },
  );
  server.registerTool(
    'test_missing_capability',
    {
      description:
        'Attempts an input request regardless of declared capabilities; the server seam must ' +
        'reject it with MissingRequiredClientCapabilityError (SEP-2575 diagnostic fixture).',
      inputSchema: z.object({}),
    },
    (_args, ctx) => {
      const view = inputResponse(ctx.mcpReq.inputResponses, 'sample');
      if (view.kind === 'sampling') {
        return text('Sampling input received.');
      }
      // When the client declared no sampling capability, the SDK's
      // inputRequired seam throws MissingRequiredClientCapabilityError
      // (-32021, HTTP 400) before this result ships — the behavior the
      // server-stateless scenario probes.
      return inputRequired({
        inputRequests: {
          sample: inputRequired.createMessage({
            messages: [
              { role: 'user', content: { type: 'text', text: 'Reply with any short text.' } },
            ],
            maxTokens: 50,
          }),
        },
      });
    },
  );

  // -- prompts ---------------------------------------------------------
  server.registerPrompt(
    'test_simple_prompt',
    { description: 'A simple prompt with no arguments (conformance fixture).' },
    () => ({
      messages: [
        {
          role: 'user' as const,
          content: { type: 'text' as const, text: 'This is a simple prompt for testing.' },
        },
      ],
    }),
  );
  server.registerPrompt(
    'test_prompt_with_arguments',
    {
      description: 'A parameterized prompt (conformance fixture).',
      argsSchema: z.object({
        arg1: z.string().describe('First test argument'),
        arg2: z.string().describe('Second test argument'),
      }),
    },
    ({ arg1, arg2 }) => ({
      messages: [
        {
          role: 'user' as const,
          content: {
            type: 'text' as const,
            text: `Prompt with arguments: arg1='${arg1}', arg2='${arg2}'`,
          },
        },
      ],
    }),
  );
  server.registerPrompt(
    'test_prompt_with_embedded_resource',
    { description: 'A prompt embedding a resource (conformance fixture).' },
    () => ({
      messages: [
        {
          role: 'user' as const,
          content: {
            type: 'resource' as const,
            resource: {
              uri: 'test://embedded-resource',
              mimeType: 'text/plain',
              text: EMBEDDED_RESOURCE_TEXT,
            },
          },
        },
      ],
    }),
  );
  server.registerPrompt(
    'test_prompt_with_image',
    { description: 'A prompt embedding an image (conformance fixture).' },
    () => ({
      messages: [
        {
          role: 'user' as const,
          content: { type: 'image' as const, data: PNG_BASE64, mimeType: 'image/png' },
        },
      ],
    }),
  );
  server.registerPrompt(
    'test_input_required_result_prompt',
    {
      description: 'A prompt that requires elicitation input (conformance fixture).',
    },
    (ctx) => {
      const answered = acceptedContent(ctx.mcpReq.inputResponses, 'user_context', contextSchema);
      if (answered !== undefined) {
        return {
          messages: [
            {
              role: 'user' as const,
              content: {
                type: 'text' as const,
                text: `Prompt input received and accepted (context: ${answered.context}).`,
              },
            },
          ],
        };
      }
      return inputRequired({ inputRequests: { user_context: USER_CONTEXT_REQUEST() } });
    },
  );

  // -- completion ------------------------------------------------------
  // The capability just needs to be declared and the endpoint must
  // respond correctly; suggestions can be minimal (completion-complete
  // scenario). We complete arg1 of test_prompt_with_arguments from a
  // fixed candidate list.
  server.server.registerCapabilities({ completions: {} });
  server.server.setRequestHandler('completion/complete', async (request) => {
    const params = request.params as {
      ref?: { type?: string; name?: string };
      argument?: { name?: string; value?: string };
    };
    const candidates = ['paris', 'park', 'party', 'parameter', 'parse'];
    const prefix = params.argument?.value ?? '';
    const values = candidates.filter((c) => c.startsWith(prefix));
    return {
      completion: {
        values,
        total: values.length,
        hasMore: false,
      },
    };
  });
}
