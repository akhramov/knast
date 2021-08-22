INSERT INTO storage (tree, key, value) VALUES (:tree, :key, :old_value) ON CONFLICT DO NOTHING;
