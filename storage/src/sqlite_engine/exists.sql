SELECT EXISTS(SELECT 1 FROM storage WHERE key = :key AND tree = :tree);
