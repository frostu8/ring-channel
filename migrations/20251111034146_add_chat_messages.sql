CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    player_id INTEGER NOT NULL REFERENCES player(id),
    content VARCHAR(255) NOT NULL,
    inserted_at TIMESTAMP NOT NULL
);
