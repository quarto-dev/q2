/**
 * Test utilities for hub-client
 *
 * This module provides shared test utilities, mocks, and helpers
 * for both unit and integration tests.
 */

// Re-export testing library utilities for convenience
export { render, screen, fireEvent, waitFor } from '@testing-library/react';

// Export mock utilities
export { createMockSyncClient } from './mockSyncClient';
export type { MockSyncClient, MockSyncClientOptions } from './mockSyncClient';

export { createMockWasmRenderer } from './mockWasm';
export type { MockWasmRenderer, MockWasmOptions, VfsResponse, RenderResult } from './mockWasm';

// Export test fixtures (will be added as needed)
// export * from './testFixtures';
