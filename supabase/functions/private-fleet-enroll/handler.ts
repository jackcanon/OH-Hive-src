import { assertion, challenge, uuid } from "./protocol.ts";
export type Fleet = { id: string; name: string };
export type Dependencies = {
  authenticate: (jwt: string) => Promise<string | null>;
  ownedFleet: (user: string, fleet: string) => Promise<Fleet | null>;
  createFleet: (user: string, name: string) => Promise<Fleet>;
  signingKey: CryptoKey; issuer: string; keyId: string; origins: string[];
};
export function createHandler(d: Dependencies) {
  return async (req: Request): Promise<Response> => {
    const origin = req.headers.get("origin");
    const headers: Record<string,string> = { "Cache-Control": "no-store", "Vary": "Origin" };
    const reply = (status: number, body: unknown) => Response.json(body, { status, headers });
    if (origin && !d.origins.includes(origin)) return reply(403,{error:"origin_not_allowed"});
    if (origin) headers["Access-Control-Allow-Origin"] = origin;
    headers["Access-Control-Allow-Headers"] = "authorization, apikey, content-type, x-client-info";
    headers["Access-Control-Allow-Methods"] = "POST, OPTIONS";
    if (req.method === "OPTIONS") return new Response(null,{status:204,headers});
    if (req.method !== "POST") return reply(405,{error:"method_not_allowed"});
    try {
      // Bound the stream, including requests without a truthful Content-Length.
      const reader=req.body?.getReader(); if (!reader) return reply(400,{error:"invalid_request"});
      const chunks: Uint8Array[]=[];let size=0;
      while(true) { const {done,value}=await reader.read();if(done)break;size+=value.length;if(size>8192){await reader.cancel();return reply(413,{error:"request_too_large"});}chunks.push(value); }
      const bytes=new Uint8Array(size);let at=0;for(const part of chunks){bytes.set(part,at);at+=part.length;}
      let body;try {body=JSON.parse(new TextDecoder().decode(bytes));}catch{return reply(400,{error:"invalid_request"});}
      if (!body || typeof body !== "object" || Array.isArray(body)) return reply(400,{error:"invalid_request"});
      const token=req.headers.get("authorization")?.match(/^Bearer (\S+)$/i)?.[1];
      if (!token) return reply(401,{error:"sign_in_required"});
      const user=await d.authenticate(token);
      if (!uuid(user)) return reply(401,{error:"sign_in_required"});
      if (body.action === "create_fleet") {
        if (Object.keys(body).sort().join() !== "action,name" || typeof body.name !== "string" || body.name.trim().length<1 || body.name.trim().length>80) return reply(400,{error:"invalid_request"});
        return reply(200,{fleet:await d.createFleet(user,body.name.trim())});
      }
      if(body.action !== "approve" || Object.keys(body).sort().join() !== "action,challenge,fleet_id" || !uuid(body.fleet_id)) return reply(400,{error:"invalid_request"});
      const now=Math.floor(Date.now()/1000);let c;
      try {c=challenge(body.challenge,now);}catch{return reply(400,{error:"invalid_or_expired_challenge"});}
      const fleet=await d.ownedFleet(user,body.fleet_id);
      if (!fleet) return reply(403,{error:"fleet_not_owned"});
      return reply(200,{assertion:await assertion(d.signingKey,d.issuer,d.keyId,user,fleet.id,c,now)});
    } catch { return reply(503,{error:"enrollment_unavailable"}); }
  };
}
