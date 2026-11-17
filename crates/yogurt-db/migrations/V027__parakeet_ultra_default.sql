-- Parakeet Ultra replaces v3 as the default meetings model for a new install: Welcome not finished, not on Local, and the seed untouched.
-- Everyone else keeps the model they had, so a v3 someone picked stays v3.
UPDATE settings SET value = 'parakeet-ultra'
    WHERE key = 'stt.model' AND value = 'parakeet-v3'
      AND EXISTS (SELECT 1 FROM settings WHERE key = 'first_run_completed' AND value = 'false')
      AND NOT EXISTS (SELECT 1 FROM settings WHERE key = 'stt.provider' AND value = 'local');
