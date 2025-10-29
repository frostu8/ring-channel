CREATE TABLE rating_period (
    id INTEGER PRIMARY KEY,
    -- When the rating period started.
    inserted_at TIMESTAMP NOT NULL
);

CREATE TABLE rating (
    id INTEGER PRIMARY KEY,
    -- The id of the player this is representing.
    -- These aren't unique! The player's canonical rating is the rating that
    -- was last inserted into the DB! These are stored for caching reasons.
    player_id INTEGER NOT NULL REFERENCES player(id),
    -- The Glicko2 stats
    rating REAL NOT NULL,
    deviation REAL NOT NULL,
    volatility REAL NOT NULL,
    inserted_at TIMESTAMP NOT NULL,
    -- Can be used to find what period this rating belongs to.
    updated_at TIMESTAMP NOT NULL
);
