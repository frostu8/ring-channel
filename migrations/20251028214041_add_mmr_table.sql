CREATE TABLE rating_period (
    id INTEGER PRIMARY KEY,
    -- When the rating period started.
    inserted_at TIMESTAMP NOT NULL
);

-- The rating of a player at the start of a rating period.
CREATE TABLE rating (
    id INTEGER PRIMARY KEY,
    period_id INTEGER NOT NULL REFERENCES rating_period(id),
    -- The id of the player this is representing.
    player_id INTEGER NOT NULL REFERENCES player(id),
    -- The Glicko2 stats
    rating REAL NOT NULL,
    deviation REAL NOT NULL,
    volatility REAL NOT NULL,
    inserted_at TIMESTAMP NOT NULL,

    UNIQUE (period_id, player_id)
);

-- Cached results from Glicko2 insanity
ALTER TABLE player ADD COLUMN rating REAL NOT NULL DEFAULT 1500.0;
ALTER TABLE player ADD COLUMN deviation REAL NOT NULL DEFAULT 350.0;
ALTER TABLE player ADD COLUMN volatility REAL NOT NULL DEFAULT 0.06;
