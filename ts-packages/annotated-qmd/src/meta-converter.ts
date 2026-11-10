/**
 * Metadata Conversion
 *
 * Converts MetaValue structures from quarto-markdown-pandoc JSON
 * into AnnotatedParse structures compatible with quarto-cli.
 */

import type { AnnotatedParse, JsonMetaValue, MetaMapEntry, JSONValue } from './types.js';
import type { SourceInfoReconstructor } from './source-map.js';

/**
 * Type guard for MetaMap content structure
 */
function isMetaMapContent(c: unknown): c is { entries: MetaMapEntry[] } {
  return (
    typeof c === 'object' &&
    c !== null &&
    'entries' in c &&
    Array.isArray((c as { entries: unknown }).entries)
  );
}

/**
 * Type guard for checking if value is an array of JsonMetaValue
 */
function isMetaValueArray(c: unknown): c is JsonMetaValue[] {
  return Array.isArray(c);
}

/**
 * Converts metadata from quarto-markdown-pandoc JSON to AnnotatedParse
 */
export class MetadataConverter {
  constructor(
    private sourceReconstructor: SourceInfoReconstructor,
    private metaTopLevelKeySources?: Record<string, number>
  ) {}

  /**
   * Convert top-level metadata object to AnnotatedParse
   */
  convertMeta(jsonMeta: Record<string, JsonMetaValue>): AnnotatedParse {
    // Create a synthetic MetaMap for the top-level metadata
    const entries: MetaMapEntry[] = Object.entries(jsonMeta).map(([key, value]) => ({
      key,
      // Use metaTopLevelKeySources if available, otherwise fall back to value's source
      key_source: this.metaTopLevelKeySources?.[key] ?? value.s,
      value
    }));

    // Find the overall range by getting min/max offsets
    // Must use getSourceLocation() to get RESOLVED top-level coordinates,
    // not getOffsets() which returns LOCAL coordinates from the pool
    // Entries no source text produced (e.g. a key the reader injects with a
    // `config-default` Generated source) have no location: they do not
    // widen the range, and they sort after every located entry.
    let minStart = Infinity;
    let maxEnd = -Infinity;
    for (const [key, value] of Object.entries(jsonMeta)) {
      // Consider both key and value positions
      const keySourceId = this.metaTopLevelKeySources?.[key] ?? value.s;
      for (const id of [keySourceId, value.s]) {
        if (!this.sourceReconstructor.hasSourceLocation(id)) continue;
        const loc = this.sourceReconstructor.getSourceLocation(id);
        minStart = Math.min(minStart, loc.start);
        maxEnd = Math.max(maxEnd, loc.end);
      }
    }

    // If no metadata, use defaults
    if (minStart === Infinity) {
      minStart = 0;
      maxEnd = 0;
    }

    // Convert to AnnotatedParse components
    // Sort entries by source position to maintain source order
    const sortedEntries: Array<{ key: string; value: JsonMetaValue; keyStart: number }> = [];

    for (const [key, value] of Object.entries(jsonMeta)) {
      const keySourceId = this.metaTopLevelKeySources?.[key] ?? value.s;
      const keyStart = this.sourceReconstructor.hasSourceLocation(keySourceId)
        ? this.sourceReconstructor.getSourceLocation(keySourceId).start
        : Infinity;
      sortedEntries.push({ key, value, keyStart });
    }

    // Sort by key start position (stable: location-less entries, keyed
    // Infinity, keep their relative order at the end)
    sortedEntries.sort((a, b) =>
      a.keyStart === b.keyStart ? 0 : a.keyStart < b.keyStart ? -1 : 1
    );

    const components: AnnotatedParse[] = [];
    const result: Record<string, JSONValue> = {};

    for (const { key, value, keyStart: sortKey } of sortedEntries) {
      // A key no source text produced is kept in `result` (the JSON value is
      // reported faithfully) but not in `components`: the AnnotatedParse
      // tree describes source text, and a location-less node would claim
      // offset 0 of the user's file.
      if (sortKey === Infinity) {
        result[key] = this.convertMetaValue(value).result;
        continue;
      }

      // Create AnnotatedParse for key
      // Use metaTopLevelKeySources if available, otherwise fall back to value's source
      const keySourceId = this.metaTopLevelKeySources?.[key] ?? value.s;
      const { source: keySource, start: keyStart, end: keyEnd } =
        this.sourceReconstructor.getAnnotatedParseSourceFields(keySourceId);

      const keyAP: AnnotatedParse = {
        result: key,
        kind: 'key',
        source: keySource,
        components: [],
        start: keyStart,
        end: keyEnd
      };

      // Create AnnotatedParse for value
      const valueAP = this.convertMetaValue(value);

      // Interleave key and value
      components.push(keyAP, valueAP);
      result[key] = valueAP.result;
    }

    // Get top-level MappedString for metadata section (file 0 is main document)
    const topSource = this.sourceReconstructor.getTopLevelMappedString(0);

    return {
      result,
      kind: 'mapping',  // Top-level metadata is a mapping
      source: topSource,
      components,
      start: minStart,
      end: maxEnd
    };
  }

  /**
   * Convert individual MetaValue to AnnotatedParse
   */
  convertMetaValue(meta: JsonMetaValue): AnnotatedParse {
    const { source, start, end } =
      this.sourceReconstructor.getAnnotatedParseSourceFields(meta.s);

    switch (meta.t) {
      case 'MetaString':
        return {
          result: typeof meta.c === 'string' ? meta.c : String(meta.c ?? ''),
          kind: 'MetaString',
          source,
          components: [],
          start,
          end
        };

      case 'MetaBool':
        return {
          result: typeof meta.c === 'boolean' ? meta.c : Boolean(meta.c),
          kind: 'MetaBool',
          source,
          components: [],
          start,
          end
        };

      case 'MetaInlines':
        return {
          result: meta.c as JSONValue,  // Array of inline JSON objects AS-IS
          kind: meta.t,  // Just use the type directly - tag info is in the structure
          source,
          components: [],  // Empty - cannot track internal locations yet
          start,
          end
        };

      case 'MetaBlocks':
        return {
          result: meta.c as JSONValue,  // Array of block JSON objects AS-IS
          kind: 'MetaBlocks',
          source,
          components: [],
          start,
          end
        };

      case 'MetaList':
        return this.convertMetaList(meta, source, start, end);

      case 'MetaMap':
        return this.convertMetaMap(meta, source, start, end);

      default:
        // Unknown type - return as-is with generic kind
        return {
          result: meta.c as JSONValue,
          kind: meta.t,
          source,
          components: [],
          start,
          end
        };
    }
  }

  /**
   * Convert MetaList to AnnotatedParse
   */
  private convertMetaList(
    meta: JsonMetaValue,
    source: ReturnType<SourceInfoReconstructor['toMappedString']>,
    start: number,
    end: number
  ): AnnotatedParse {
    // Runtime type check
    if (!isMetaValueArray(meta.c)) {
      // Return empty list if content is not an array
      return {
        result: [],
        kind: 'MetaList',
        source,
        components: [],
        start,
        end
      };
    }

    const items = meta.c.map(item => this.convertMetaValue(item));

    return {
      result: items.map(item => item.result),
      kind: 'MetaList',
      source,
      components: items,
      start,
      end
    };
  }

  /**
   * Convert MetaMap to AnnotatedParse with interleaved key/value components
   */
  private convertMetaMap(
    meta: JsonMetaValue,
    source: ReturnType<SourceInfoReconstructor['toMappedString']>,
    start: number,
    end: number
  ): AnnotatedParse {
    // Runtime type check
    if (!isMetaMapContent(meta.c)) {
      // Return empty map if content is not valid
      return {
        result: {},
        kind: 'MetaMap',
        source,
        components: [],
        start,
        end
      };
    }

    const entries = meta.c.entries;
    const components: AnnotatedParse[] = [];
    const result: Record<string, JSONValue> = {};

    for (const entry of entries) {
      const { source: keySource, start: keyStart, end: keyEnd } =
        this.sourceReconstructor.getAnnotatedParseSourceFields(entry.key_source);

      const keyAP: AnnotatedParse = {
        result: entry.key,
        kind: 'key',
        source: keySource,
        components: [],
        start: keyStart,
        end: keyEnd
      };

      const valueAP = this.convertMetaValue(entry.value);

      // Interleave key and value in components (matches js-yaml pattern)
      components.push(keyAP, valueAP);
      result[entry.key] = valueAP.result;
    }

    return {
      result,
      kind: 'MetaMap',
      source,
      components,
      start,
      end
    };
  }
}
