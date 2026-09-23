// @vitest-environment jsdom
/**
 * Cross-consumer conformance test between the canonical custom-node
 * schema (`crates/quarto-pandoc-types/resources/custom-node-schema.json`,
 * P2 Task 1) and the real TS consumers of that wire format: the
 * `previewRegistry` / `Custom.*` barrel (T4.1), the unrecognized-type
 * fallback path (T4.2), and each component's `plain_data` key array
 * (T4.3). See P2 Task 4 (pandoc-hybrid P2 implementation plan).
 *
 * `registry.test.ts:81-95` already checks one direction — every name
 * in a hardcoded list is exported from `./custom`. It is a *subset*
 * assertion: a 9th type added to the schema with no TS component still
 * passes it. T4.1 supplies the missing direction: schema-reachable
 * types === registered types, exactly.
 */

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { createElement } from 'react';
import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import * as Custom from './custom';
import { previewRegistry } from './registry';
import { Ast } from '../framework';
import type { PandocAST } from '../framework';
import { CALLOUT_PLAIN_DATA_KEYS } from './custom/Callout';
import { THEOREM_PLAIN_DATA_KEYS } from './custom/Theorem';
import { PROOF_PLAIN_DATA_KEYS } from './custom/Proof';
import { FLOAT_REF_TARGET_PLAIN_DATA_KEYS } from './custom/FloatRefTarget';
import { EQUATION_PLAIN_DATA_KEYS } from './custom/Equation';
import { CROSSREF_RESOLVED_REF_PLAIN_DATA_KEYS } from './custom/CrossrefResolvedRef';

const here = dirname(fileURLToPath(import.meta.url));
const schemaPath = resolve(here, '../../../../crates/quarto-pandoc-types/resources/custom-node-schema.json');

interface SchemaFieldSpec {
    required: boolean;
}

interface SchemaTypeSpec {
    route: string;
    slots: Record<string, string>;
    plain_data: Record<string, SchemaFieldSpec>;
}

interface CustomNodeSchema {
    version: number;
    types: Record<string, SchemaTypeSpec>;
}

function loadSchema(): CustomNodeSchema {
    return JSON.parse(readFileSync(schemaPath, 'utf-8')) as CustomNodeSchema;
}

// Types in the schema with no `Custom.*` component. Each entry must carry
// a code citation. `ExampleEmbed` is not in the schema at all (Task 1
// acceptance item 5), so it is not listed here.
const EXPECTED_UNREACHABLE: Record<string, string> = {
    // Excluded pre-creation by Q2_PREVIEW_TRANSFORM_EXCLUDED
    // (pipeline.rs:1637-1638) — no `CustomNode("Tabset")` ever reaches
    // q2-preview's pipeline for this to route.
    Tabset: 'pipeline.rs:1637-1638 (Q2_PREVIEW_TRANSFORM_EXCLUDED)',
};

// Schema-reachable set: every schema type with a `Custom.*` component, i.e.
// every schema type name minus `EXPECTED_UNREACHABLE`. Hoisted out of T4.1's
// `it` body (module scope, computed once at load) so both T4.1 and T4.3's
// own-key-set check below can share it as the same source of truth.
const schemaForReachability = loadSchema();
const schemaTypeNames = new Set(Object.keys(schemaForReachability.types));
const reachableFromSchema = new Set(
    [...schemaTypeNames].filter((t) => !(t in EXPECTED_UNREACHABLE)),
);

describe('schema conformance: previewRegistry vs custom-node-schema.json', () => {
    it('EXPECTED_UNREACHABLE has exactly one entry (growing it is a deliberate edit)', () => {
        expect(Object.keys(EXPECTED_UNREACHABLE).length).toBe(1);
    });

    it('T4.1: schema-reachable type set equals the registered Custom.* set, exactly', () => {
        const customExportNames = new Set(Object.keys(Custom));
        const exportedAndInSchema = new Set(
            [...customExportNames].filter((n) => schemaTypeNames.has(n)),
        );

        expect(reachableFromSchema).toEqual(exportedAndInSchema);

        // And each is registered as a callable component on previewRegistry.
        for (const typeName of reachableFromSchema) {
            expect(typeof (previewRegistry as Record<string, unknown>)[typeName]).toBe(
                'function',
            );
        }
    });
});

function astJson(blocks: unknown[]): string {
    const ast: PandocAST = {
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: blocks as PandocAST['blocks'],
    };
    return JSON.stringify(ast);
}

describe('T4.2: unrecognized type_name falls back to Fallback, does not throw', () => {
    it('a CustomBlock with type_name "Tabset" (real schema type, no registry entry) renders via Fallback', () => {
        const blocks = [
            {
                t: 'CustomBlock',
                type_name: 'Tabset',
                slots: {
                    'title-0': { kind: 'inlines', value: [{ t: 'Str', c: 'Tab 1' }] },
                    'content-0': {
                        kind: 'blocks',
                        value: [{ t: 'Para', c: [{ t: 'Str', c: 'body' }] }],
                    },
                },
                plain_data: { level: 3, tab_count: 1, actives: [true] },
                attr: ['', [], []],
            },
        ];

        expect(() =>
            render(
                createElement(Ast, {
                    astJson: astJson(blocks),
                    currentFilePath: '/project/test.qmd',
                    onNavigateToDocument: () => {},
                    setAst: () => {},
                    registry: previewRegistry,
                }),
            ),
        ).not.toThrow();

        // Fallback renders the unrecognized type_name in a styled badge.
        // `getByText` throws if no matching element exists, which is
        // itself the assertion (this default tier has no jest-dom matchers).
        expect(screen.getByText('Tabset')).toBeTruthy();
        expect(screen.getByText('body')).toBeTruthy();
    });
});

// T4.3 ————————————————————————————————————————————————————————————————
// Per-type plain_data key arrays vs. the schema's plain_data key set,
// asserted in both directions.

const COMPONENT_KEY_ARRAYS: Record<string, readonly string[]> = {
    Callout: CALLOUT_PLAIN_DATA_KEYS,
    Theorem: THEOREM_PLAIN_DATA_KEYS,
    Proof: PROOF_PLAIN_DATA_KEYS,
    FloatRefTarget: FLOAT_REF_TARGET_PLAIN_DATA_KEYS,
    Equation: EQUATION_PLAIN_DATA_KEYS,
    CrossrefResolvedRef: CROSSREF_RESOLVED_REF_PLAIN_DATA_KEYS,
};

describe('T4.3: component plain_data key arrays vs. schema plain_data key sets', () => {
    const schema = loadSchema();

    it("COMPONENT_KEY_ARRAYS' own key set equals the schema-reachable set (every reachable type has a plain_data array, and vice versa)", () => {
        // Every type in `reachableFromSchema` (T4.1: schema type with a
        // `Custom.*` component) has a non-empty `plain_data` in the schema
        // today (see custom-node-schema.json), so the reachable set and the
        // "has both a component and plain_data fields" set coincide. This
        // assertion is what stops a 7th schema type from getting a
        // `Custom.*` component and its own `*_PLAIN_DATA_KEYS` export while
        // a contributor forgets to add it here — without it, the per-type
        // loop below silently skips the new type.
        expect(new Set(Object.keys(COMPONENT_KEY_ARRAYS))).toEqual(reachableFromSchema);
    });

    for (const [typeName, keyArray] of Object.entries(COMPONENT_KEY_ARRAYS)) {
        it(`${typeName}: key array equals the schema's plain_data key set`, () => {
            const schemaKeys = Object.keys(schema.types[typeName].plain_data);
            expect([...keyArray].sort()).toEqual([...schemaKeys].sort());
        });
    }
});
