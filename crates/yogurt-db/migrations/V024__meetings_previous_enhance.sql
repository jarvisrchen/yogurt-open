-- The enhanced notes a re-enhance replaced, as JSON {enriched_md, llm_model, template}, until the user keeps one version.
ALTER TABLE meetings ADD COLUMN previous_enhance TEXT;
