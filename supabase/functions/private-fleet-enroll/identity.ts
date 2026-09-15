/** Input must come from Supabase auth.getUser(jwt), never decoded caller-supplied JWT JSON. */
export function platformSubject(user: { id: string; is_anonymous?: boolean; identities?: { provider: string }[] } | null): string | null {
  return user && !user.is_anonymous && user.identities?.some(identity => identity.provider === "google" || identity.provider === "apple") ? user.id : null;
}
