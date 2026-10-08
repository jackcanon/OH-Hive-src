-- About 22,500 expiring samples/day; default 20% threshold waits ~76,000 dead rows.
-- Trigger routine maintenance near 15,700 dead rows instead, with bounded cost.
ALTER TABLE hive.rtt_samples SET (
 autovacuum_vacuum_scale_factor=0.04,
 autovacuum_vacuum_threshold=500,
 autovacuum_analyze_scale_factor=0.02,
 autovacuum_analyze_threshold=500
);
-- Rollback: ALTER TABLE hive.rtt_samples RESET (autovacuum_vacuum_scale_factor,
-- autovacuum_vacuum_threshold,autovacuum_analyze_scale_factor,autovacuum_analyze_threshold);
