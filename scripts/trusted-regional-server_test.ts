import { isTrustedRegionalServer } from "../apps/web/lib/trusted-regional-server.ts";
Deno.test("member bearer tokens only go to online hub-attested HJM HTTPS servers", () => {
  const trusted = { operator: "hjm", status: "online", public_url: "https://regional.example.com" };
  if (!isTrustedRegionalServer(trusted)) throw new Error("Trusted server rejected");
  for (const patch of [{ operator: "volunteer" }, { operator: undefined }, { operator: "HJM" }, { status: "offline" }, { public_url: null }, { public_url: "http://regional.example.com" }, { public_url: "https://user:password@regional.example.com" }, { public_url: "invalid" }]) {
    if (isTrustedRegionalServer({ ...trusted, ...patch })) throw new Error("Untrusted server accepted");
  }
});
