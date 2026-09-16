import { createClient } from "npm:@supabase/supabase-js@2";
import { createHandler } from "./handler.ts";
import { decode } from "./protocol.ts";
import { platformSubject } from "./identity.ts";
const required=(name:string)=>{const value=Deno.env.get(name);if(!value)throw new Error(`Missing ${name}`);return value;};
const url=required("SUPABASE_URL");
const admin=createClient(url,required("SUPABASE_SERVICE_ROLE_KEY"),{auth:{persistSession:false,autoRefreshToken:false}});
const signingKey=await crypto.subtle.importKey("pkcs8",decode(required("PRIVATE_FLEET_SIGNING_KEY_PKCS8")),"Ed25519",false,["sign"]);
const issuer=required("PRIVATE_FLEET_ISSUER");if(new URL(issuer).protocol!=="https:")throw new Error("Issuer must be HTTPS");
Deno.serve(createHandler({
  issuer,keyId:required("PRIVATE_FLEET_SIGNING_KEY_ID"),signingKey,
  origins:required("PRIVATE_FLEET_ALLOWED_ORIGINS").split(",").map(v=>v.trim()),
  authenticate:async jwt=>{const {data,error}=await admin.auth.getUser(jwt);return error ? null : platformSubject(data.user);},
  ownedFleet:async(owner,id)=>{const {data,error}=await admin.schema("hive").from("private_fleets").select("id,name").eq("owner_id",owner).eq("id",id).maybeSingle();if(error)throw error;return data;},
  createFleet:async(owner,name)=>{const {data,error}=await admin.schema("hive").from("private_fleets").insert({owner_id:owner,name}).select("id,name").single();if(error)throw error;return data;},
}));
