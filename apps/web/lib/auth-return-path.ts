/** OAuth returns may navigate only to a path on this website. */
export function safeAuthReturnPath(value: string | null, origin: string): string {
  if (!value?.startsWith("/") || value.includes("\\")) return "/";
  try {
    const url = new URL(value, origin);
    return url.origin === origin ? url.pathname + url.search + url.hash : "/";
  } catch { return "/"; }
}
