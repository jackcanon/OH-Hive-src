// Temporary bearer-token boundary until project-scoped live tokens replace member JWTs.
// `operator` is hub-attested (server_register restricts 'hjm' to the founder account).
export function isTrustedRegionalServer<T extends { operator?: string; status: string; public_url: string | null }>(server: T): server is T & { public_url: string } {
  if (server.operator !== "hjm" || server.status !== "online" || !server.public_url) return false;
  try {
    const url = new URL(server.public_url);
    return url.protocol === "https:" && !url.username && !url.password;
  } catch { return false; }
}
