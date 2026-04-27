/**
 * @quarto/api
 *
 * Quarto engine author API.
 * This is the aggregate entry point — subpath exports (./config, etc.) are the
 * primary surfaces; this re-exports them for convenience.
 *
 * Factory-naming convention for Plan-2 aggregators:
 *   - Fully-host namespaces (all I/O goes through PlatformHost) export a
 *     single `make<Ns>(host)` factory: `makeConsole(host)`, `makeSystem(host)`.
 *   - Mostly-pure namespaces (only one or two functions need the host) export
 *     their pure functions directly and expose only the host-dependent portion
 *     via a `make<Ns>Host(host)` factory: `makePathHost(host)`,
 *     `makeMappedStringHost(host)`.
 *
 * A Plan-2 aggregator should call `make<Ns>` for the fully-host namespaces
 * and call `make<Ns>Host` for the mostly-pure namespaces (then mix the result
 * with the namespace's pure exports to produce the combined surface).
 */

export * from "./config/index.js";
export * from "./platform/index.js";
export * from "./text/index.js";
export * from "./format/index.js";
export * from "./crypto/index.js";
export * from "./mappedString/index.js";
export * from "./markdownRegex/index.js";
export * from "./console/index.js";
export * from "./path/index.js";
export * from "./system/index.js";
