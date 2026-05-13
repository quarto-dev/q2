// q2-preview iframe entry — re-imports the real entry from the shared
// `@quarto/preview-renderer` workspace package. The entry was moved out
// of hub-client by bd-hfjj Phase 4 but the script tag in
// `hub-client/q2-preview.html` (and the parity integration test in
// `parity.integration.test.tsx`) keep the original path stable by
// going through this one-line stub.
import '@quarto/preview-renderer/q2-preview/entry';
