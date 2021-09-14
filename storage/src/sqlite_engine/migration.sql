CREATE TABLE IF NOT EXISTS storage(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tree BLOB NOT NULL,
    key BLOB NOT NULL,
    value BLOB,
    UNIQUE (tree, key)
);