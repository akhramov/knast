INSERT INTO storage (tree, key, value) VALUES (:tree, :key, :value) ON CONFLICT
DO UPDATE
SET value = EXCLUDED.value;
