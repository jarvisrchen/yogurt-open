-- V019 later replaces retired models and updates untouched onboarding defaults.
-- Seed missing keys without overwriting saved transcription choices.

INSERT OR IGNORE INTO settings(key, value) VALUES
    ('stt.provider', 'cloud'),
    ('stt.model',    'small.en');
