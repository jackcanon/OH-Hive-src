-- Agent bios and the owner's "About you" profile (Sif, 2026-09-18).
--
-- These arrived on her branch as two bare CREATE TABLE IF NOT EXISTS statements executed
-- unconditionally on every open, sitting in the body of the counter ladder rather than as a step
-- of it. That worked, but it was outside the migration system: nothing recorded that it had run,
-- and nothing would have stopped an older binary opening a database that had them.
--
-- The merge that brought her work onto the named-migration engine deleted the ladder those two
-- statements lived in, and a textual merge would have dropped them silently -- the app would have
-- built, installed, and then failed at runtime on "no such table: bots_agent_bios". They are a
-- migration now, with a name, like everything else.
--
-- IF NOT EXISTS is kept deliberately: her build already created both tables on the live vault, so
-- this has to be a no-op there rather than an error.
CREATE TABLE IF NOT EXISTS bots_agent_bios(
  agent        TEXT PRIMARY KEY,
  bio          TEXT NOT NULL,
  instructions TEXT NOT NULL,
  avatar       TEXT NOT NULL,
  revision     INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS bots_user_profiles(
  owner          TEXT PRIMARY KEY,
  preferred_name TEXT NOT NULL,
  about          TEXT NOT NULL
);
