-- MiniMax retired Text-01 and the abab models, and its API now rejects them, so a MiniMax row naming one moves to MiniMax-M3.
UPDATE providers SET model = 'MiniMax-M3'
    WHERE adapter = 'http' AND base_url LIKE '%api.minimax.io%'
      AND lower(model) IN ('minimax-text-01', 'minimax-text-01-250515', 'abab6.5s-chat', 'abab6.5-chat');
