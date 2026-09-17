export type DesktopBridge = { request: string; state: string; port: number };
export function parseDesktopBridge(encoded: string, now = Date.now() / 1000): DesktopBridge | null {
  try {
    if (encoded.length > 12000) return null;
    const bridge = JSON.parse(atob(encoded));
    if (typeof bridge.request !== "string" || bridge.request.length > 8000 ||
        typeof bridge.state !== "string" || !/^[a-f0-9-]{36}$/i.test(bridge.state) ||
        !Number.isInteger(bridge.port) || bridge.port < 1024 || bridge.port > 65535) return null;
    const request = JSON.parse(bridge.request);
    if (!request || !Number.isSafeInteger(request.expires_at) || request.expires_at <= now || request.expires_at > now + 600 ||
        typeof request.authority_id !== "string" || typeof request.node_id !== "string" ||
        typeof request.nonce !== "string" || !/^[a-f0-9]{64}$/.test(request.credential_sha256)) return null;
    return bridge;
  } catch { return null; }
}
export function desktopCallback(bridge: DesktopBridge, assertion: unknown): string {
  if (!Number.isInteger(bridge.port) || bridge.port < 1024 || bridge.port > 65535 ||
      !/^[a-f0-9-]{36}$/i.test(bridge.state)) throw new Error("Invalid callback destination");
  const json = JSON.stringify(assertion);
  if (!json || new TextEncoder().encode(json).length > 8000) throw new Error("Invalid enrollment response");
  const bytes = new TextEncoder().encode(json);
  const code = btoa(Array.from(bytes, b => String.fromCharCode(b)).join(""));
  const url = new URL(`http://127.0.0.1:${bridge.port}/fleet-enrollment`);
  url.searchParams.set("state", bridge.state);
  url.searchParams.set("approval", code);
  return url.href;
}
