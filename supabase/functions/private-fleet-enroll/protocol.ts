export const DOMAIN = "hive.private-fleet.enrollment.v1\n";
export const uuid = (v: unknown): v is string => typeof v === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v) && v !== "00000000-0000-0000-0000-000000000000";
export type Challenge = { authority_id: string; node_id: string; credential_sha256: string; nonce: string; expires_at: number };
export function challenge(v: unknown, now: number): Challenge {
  if (!v || typeof v !== "object" || Array.isArray(v)) throw new Error("invalid_challenge");
  const c = v as Challenge;
  if (Object.keys(c).sort().join() !== "authority_id,credential_sha256,expires_at,node_id,nonce" ||
    !uuid(c.authority_id) || !uuid(c.node_id) || !/^[a-f0-9]{64}$/.test(c.credential_sha256) ||
    !/^[a-f0-9]{64}$/.test(c.nonce) || !Number.isSafeInteger(c.expires_at) || c.expires_at <= now || c.expires_at > now + 330) throw new Error("invalid_challenge");
  return c;
}
export function encode(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes)).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}
export function decode(value: string): Uint8Array<ArrayBuffer> {
  return Uint8Array.from(atob(value.replaceAll("-", "+").replaceAll("_", "/")), c => c.charCodeAt(0));
}
export async function assertion(key: CryptoKey, issuer: string, keyId: string, subject: string, fleetId: string, c: Challenge, now: number) {
  const claims = { issuer, key_id: keyId, audience: "hive-private-fleet-enrollment", subject, fleet_id: fleetId,
    ...c, assertion_id: crypto.randomUUID(), issued_at: now, expires_at: Math.min(c.expires_at, now + 300) };
  const payload = encode(new TextEncoder().encode(JSON.stringify(claims)));
  const signature = encode(new Uint8Array(await crypto.subtle.sign("Ed25519", key, new TextEncoder().encode(DOMAIN + payload))));
  return { payload, signature };
}
