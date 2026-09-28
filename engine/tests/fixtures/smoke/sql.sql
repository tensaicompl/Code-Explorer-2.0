CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE VIEW active_users AS SELECT id, name FROM users;
