CREATE TABLE map_config (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER NOT NULL REFERENCES server(id),
    lumpname VARCHAR(255) NOT NULL,
    status INTEGER NOT NULL,
    note VARCHAR(2048),
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,

    UNIQUE (parent_id, lumpname)
);
