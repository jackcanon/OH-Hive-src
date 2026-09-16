-- Commit the enum extension before the pricing migration uses its new value.
alter type hive.rate_kind add value if not exists 'speech_compute_second';
