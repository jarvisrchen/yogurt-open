-- medium.en and large-v3 are gone from the model list, so a setting naming either moves to large-v3-turbo.
-- Their files stay on disk until the user deletes them in Settings.
UPDATE settings SET value = 'large-v3-turbo'
    WHERE key IN ('stt.model', 'general.voice_stt_model') AND value IN ('medium.en', 'large-v3');
-- Parakeet v3 is the default meetings model for a new install: Welcome not finished, still on Cloud, and the V005 seed untouched.
-- Everyone else keeps the model they had, including someone who picked Local in Settings before finishing Welcome.
UPDATE settings SET value = 'parakeet-v3'
    WHERE key = 'stt.model' AND value = 'small.en'
      AND EXISTS (SELECT 1 FROM settings WHERE key = 'first_run_completed' AND value = 'false')
      AND NOT EXISTS (SELECT 1 FROM settings WHERE key = 'stt.provider' AND value = 'local');
