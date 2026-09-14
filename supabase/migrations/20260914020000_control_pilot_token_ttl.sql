-- Allow an operator to shorten the isolated pilot's token TTL for lifecycle tests.
-- The database independently enforces 30..900 seconds; default remains 900.
-- Only the private gateway's auth/renew token paths use this parameter.
do $$
declare definition text;
begin
 select pg_get_functiondef('hive.ctl_pilot_call(text,uuid,text,jsonb)'::regprocedure) into definition;
 if position('token_ttl interval' in definition)>0 then return; end if;
 if position('leases uuid[];' in definition)=0 or position('now()+interval ''15 minutes''' in definition)=0 then
  raise exception 'unexpected_pilot_gateway_definition';
 end if;
 definition:=replace(definition,'leases uuid[];',
  'leases uuid[]; token_ttl interval := make_interval(secs=>greatest(30,least(coalesce((params->>''token_ttl_seconds'')::int,900),900)));');
 definition:=replace(definition,'now()+interval ''15 minutes''','now()+token_ttl');
 execute definition;
end $$;
