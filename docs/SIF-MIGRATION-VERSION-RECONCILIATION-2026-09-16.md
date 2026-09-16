# Migration version reconciliation — 2026-09-16

Read-only production version/name inventory; no statements or application data exported. Name matches below are candidates, not proof of identical migration bodies. Do not run automatic repair or push based only on this list.

| Repository migration | Production candidate | Status |
| --- | --- | --- |
| `20260905000001_hive_schema` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260905000002_node_keys_and_presence` | `20260905204509_hive_node_keys_and_presence` | Compare bodies before repairing IDs |
| `20260905000003_pairing` | `20260905205021_hive_pairing` | Compare bodies before repairing IDs |
| `20260905000004_dispatch_and_ledger` | `20260905221330_hive_dispatch_and_ledger` | Compare bodies before repairing IDs |
| `20260905000005_member_views` | `20260905222123_hive_member_views` | Compare bodies before repairing IDs |
| `20260905000006_interview` | `20260905222609_hive_interview` | Compare bodies before repairing IDs |
| `20260905000007_housekeeping` | `20260905223148_hive_housekeeping` | Compare bodies before repairing IDs |
| `20260905000008_checkpoints` | `20260905223439_hive_checkpoints` | Compare bodies before repairing IDs |
| `20260905000009_invites` | `20260905224228_hive_invites` | Compare bodies before repairing IDs |
| `20260905000010_status` | `20260905224855_hive_status` | Compare bodies before repairing IDs |
| `20260905000011_realtime_drop` | `20260906005202_hive_realtime_drop` | Compare bodies before repairing IDs |
| `20260905000012_schema_guards` | `20260906005231_hive_schema_guards` | Compare bodies before repairing IDs |
| `20260905000013_honey_sources` | `20260906010100_hive_honey_sources` | Compare bodies before repairing IDs |
| `20260905000014_interview_local` | `20260906010730_hive_interview_local` | Compare bodies before repairing IDs |
| `20260905000015_release_card` | `20260906012317_hive_release_card` | Compare bodies before repairing IDs |
| `20260905000016_regional_servers_v0` | `20260906013552_hive_regional_servers_v0` | Compare bodies before repairing IDs |
| `20260905000017_coordinator_election` | `20260906014055_hive_coordinator_election` | Compare bodies before repairing IDs |
| `20260905000018_snapshot` | `20260906041055_hive_snapshot` | Compare bodies before repairing IDs |
| `20260905000019_storage_settlement` | `20260906041813_hive_storage_settlement` | Compare bodies before repairing IDs |
| `20260905000020_replication` | `20260906042224_hive_replication` | Compare bodies before repairing IDs |
| `20260905000021_backups` | `20260906061855_hive_backups` | Compare bodies before repairing IDs |
| `20260905000022_gc` | `20260906063150_hive_gc` | Compare bodies before repairing IDs |
| `20260905000023_status_backup` | `20260906063427_hive_status_backup` | Compare bodies before repairing IDs |
| `20260905000024_node_summary` | `20260906140706_hive_node_summary` | Compare bodies before repairing IDs |
| `20260905000025_model_ladder` | `20260906144520_hive_model_ladder` | Compare bodies before repairing IDs |
| `20260905000026_interview_provider` | `20260906145139_hive_interview_provider` | Compare bodies before repairing IDs |
| `20260906000027_pair_poll_trust_fields` | `20260906164640_hive_pair_poll_trust_fields` | Compare bodies before repairing IDs |
| `20260906000028_member_keys_add_nous` | `20260906171219_hive_member_keys_add_nous` | Compare bodies before repairing IDs |
| `20260906000029_ledger_archival` | `20260906174124_ledger_archival` | Compare bodies before repairing IDs |
| `20260907000030_region_aware_replication` | `20260907052109_region_aware_replication` | Compare bodies before repairing IDs |
| `20260907204800_hive_add_waiting_on_child_card_status` | `20260907204727_hive_add_waiting_on_child_card_status` | Compare bodies before repairing IDs |
| `20260907204830_hive_sub_delegation_pause_resume` | `20260907204749_hive_sub_delegation_pause_resume` | Compare bodies before repairing IDs |
| `20260907234700_project_contributors` | `20260907234655_project_contributors` | Compare bodies before repairing IDs |
| `20260908000200_project_board_node_role` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260908000300_invite_welcome_grant` | `20260908051847_invite_welcome_grant` | Compare bodies before repairing IDs |
| `20260908010000_project_forum` | `20260908154229_project_forum` | Compare bodies before repairing IDs |
| `20260908030000_local_execution_mode` | `20260908181519_local_execution_mode` | Compare bodies before repairing IDs |
| `20260910000400_shard_plan_column` | `20260910195003_shard_plan_column` | Compare bodies before repairing IDs |
| `20260910000500_tos_acceptance` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260910000600_interview_prompt_plain_chat` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260912190000_hosted_media_generation` | `20260912182316_hosted_media_generation` | Compare bodies before repairing IDs |
| `20260912200000_feature_requests` | `20260912182539_feature_requests` | Compare bodies before repairing IDs |
| `20260912210000_admin_section` | `20260912184137_admin_section` | Compare bodies before repairing IDs |
| `20260912230000_member_directory` | `20260912194022_member_directory` | Compare bodies before repairing IDs |
| `20260912240000_avatar_upload` | `20260912195544_avatar_upload` | Compare bodies before repairing IDs |
| `20260912250000_node_avatars` | `20260912202641_node_avatars` | Compare bodies before repairing IDs |
| `20260912260000_feature_request_admin_status` | `20260912204913_feature_request_admin_status` | Compare bodies before repairing IDs |
| `20260912270000_node_schedules` | `20260912210016_node_schedules` | Compare bodies before repairing IDs |
| `20260912280000_bug_reports` | `20260912212925_bug_reports` | Compare bodies before repairing IDs |
| `20260912290000_provider_purchased_only` | `20260912230433_provider_purchased_only` | Compare bodies before repairing IDs |
| `20260912300000_chat_local_and_byok_only` | `20260912230758_chat_local_and_byok_only` | Compare bodies before repairing IDs |
| `20260912320000_hub_rtt_metrics` | `20260913005049_hub_rtt_metrics` | Compare bodies before repairing IDs |
| `20260912330000_node_summary_rtt` | `20260913010158_node_summary_rtt` | Compare bodies before repairing IDs |
| `20260913000000_bug_report_create_node` | `20260913172417_bug_report_create_node` | Compare bodies before repairing IDs |
| `20260913010000_chat_memory` | `20260913182139_chat_memory` | Compare bodies before repairing IDs |
| `20260913020000_personal_channel` | `20260913184331_personal_channel` | Compare bodies before repairing IDs |
| `20260913030000_notification_events_backfill_and_channel_wiring` | `20260913185209_notification_events_backfill_and_channel_wiring` | Compare bodies before repairing IDs |
| `20260913040000_channel_wiring_checkin_checkout` | `20260913185400_channel_wiring_checkin_checkout` | Compare bodies before repairing IDs |
| `20260913050000_channel_wiring_claim_and_servers` | `20260913190327_channel_wiring_claim_and_servers` | Compare bodies before repairing IDs |
| `20260913060000_personal_channel_node_key` | `20260913190514_personal_channel_node_key` | Compare bodies before repairing IDs |
| `20260913070000_channel_backfill_already_online` | `20260913191652_channel_backfill_already_online` | Compare bodies before repairing IDs |
| `20260913080000_release_notes` | `20260913192217_release_notes` | Compare bodies before repairing IDs |
| `20260913090000_member_mcp_servers` | `20260913193658_member_mcp_servers` | Compare bodies before repairing IDs |
| `20260913100000_code_modality_gate` | `20260913200644_code_modality_gate` | Compare bodies before repairing IDs |
| `20260913110000_channel_post_node_event` | `20260913200744_channel_post_node_event` | Compare bodies before repairing IDs |
| `20260913170000_node_member_dependency` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260913220000_notification_delivery_baseline` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260915120000_hive_card_submit_node_rpcs` | `20260915144511_hive_card_submit_node_rpcs` | Compare bodies before repairing IDs |
| `20260915130000_private_fleets` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260915140000_node_account_summary` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260915150000_fail_card_requires_owned_lease` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260915160000_project_scoped_live_tokens` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260915170000_hive_permissions_boundary` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260915180000_debit_and_lease_locks` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260915190000_snapshot_and_upload_urls` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260916050000_speech_rate_kind` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260916050100_speech_honey_pricing` | None by name | New local work or missing name mapping; inspect before deployment |
| `20260916050200_speech_reservations` | None by name | New local work or missing name mapping; inspect before deployment |

Production records confirm `control_pilot_delegation` at `20260914030000` and `hive_card_submit_node_rpcs` at `20260915144511`. Their old proposed/unapplied comments were stale; the files must stay in the replay chain.

Update: Jack approved the schema-definition export. Live object/function recovery and comparison are complete locally; see `SIF-LIVE-SCHEMA-RECOVERY-2026-09-16.md`. Production migration-history repair remains an unapplied rollout step: these name-based candidates must not be used for automatic repair.
