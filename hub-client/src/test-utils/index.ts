/**
 * Test utilities for hub-client.
 *
 * The `mockSyncClient` and `mockWasm` helpers moved with the services
 * they exercise in bd-hfjj Phase 5; consume them from
 * `@quarto/preview-runtime/test-utils/mockSyncClient` /
 * `@quarto/preview-runtime/test-utils/mockWasm` instead. This file
 * keeps only the hub-client-specific helpers.
 */

export { render, screen, fireEvent, waitFor } from '@testing-library/react';

export { setVisibility, resetVisibility, fireWindowFocus } from './visibility';
