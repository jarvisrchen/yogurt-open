-- Names enhance suggested for a meeting still on its default title, as a JSON array, best first.
-- NULL until the first suggestion, and '[]' once the user dismissed them or set a title, which stops further suggestions.
ALTER TABLE meetings ADD COLUMN title_suggestions TEXT;
