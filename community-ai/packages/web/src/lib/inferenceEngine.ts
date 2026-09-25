/**
 * LEGACY PWA template engine — disabled on the production path.
 * Real tokens come from llama.cpp over the native QUIC mesh.
 */
export function generateModelResponse(_prompt: string, _modelName: string = "Community AI"): string {
  throw new Error(
    "Template inference is disabled. Use native community-daemon + llama.cpp over QUIC."
  );
}
