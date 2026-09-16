-- Recovered 2026-09-16 from production information_schema and pg_constraint metadata.
-- No member data or function bodies included. Precedes the existing service-role grant.
create table if not exists hive.notification_subscriptions (
  id uuid not null default gen_random_uuid(),
  member_id uuid not null,
  channel text not null,
  external_chat_id text not null,
  project_id uuid,
  event_types text[] not null default ARRAY['card_completed'::text, 'card_failed'::text, 'member_joined'::text, 'stats_digest'::text, 'longest_hop'::text],
  created_at timestamp with time zone not null default now(),
  constraint notification_subscriptions_channel_check CHECK ((channel = ANY (ARRAY['telegram'::text, 'discord'::text, 'slack'::text]))),
  constraint notification_subscriptions_channel_external_chat_id_project_key UNIQUE (channel, external_chat_id, project_id),
  constraint notification_subscriptions_member_id_fkey FOREIGN KEY (member_id) REFERENCES hive.members(id) ON DELETE CASCADE,
  constraint notification_subscriptions_pkey PRIMARY KEY (id),
  constraint notification_subscriptions_project_id_fkey FOREIGN KEY (project_id) REFERENCES hive.projects(id) ON DELETE CASCADE
);
alter table hive.notification_subscriptions enable row level security;
revoke all on hive.notification_subscriptions from public,anon,authenticated;
grant select on hive.notification_subscriptions to authenticated;

create table if not exists hive.notification_deliveries (
  id uuid not null default gen_random_uuid(),
  event_id uuid not null,
  subscription_id uuid not null,
  status text not null default 'pending'::text,
  sent_at timestamp with time zone,
  error text,
  created_at timestamp with time zone not null default now(),
  constraint notification_deliveries_event_id_fkey FOREIGN KEY (event_id) REFERENCES hive.notification_events(id) ON DELETE CASCADE,
  constraint notification_deliveries_pkey PRIMARY KEY (id),
  constraint notification_deliveries_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'sent'::text, 'error'::text]))),
  constraint notification_deliveries_subscription_id_fkey FOREIGN KEY (subscription_id) REFERENCES hive.notification_subscriptions(id) ON DELETE CASCADE
);
alter table hive.notification_deliveries enable row level security;
revoke all on hive.notification_deliveries from public,anon,authenticated;
grant select on hive.notification_deliveries to authenticated;
grant select,insert,update,delete on hive.notification_deliveries to service_role;

CREATE INDEX IF NOT EXISTS notification_deliveries_pending_idx ON hive.notification_deliveries USING btree (status) WHERE (status = 'pending'::text);
CREATE INDEX IF NOT EXISTS notification_subscriptions_member_idx ON hive.notification_subscriptions USING btree (member_id);
