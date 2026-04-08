PRAGMA defer_foreign_keys = ON;

-- Create the extra column
ALTER TABLE rating ADD COLUMN extra TEXT;

-- Save any volatility in the extra column
UPDATE rating
SET extra = CONCAT('(volatility:', CAST(volatility AS TEXT), ')');

-- Drop volatility
ALTER TABLE rating DROP COLUMN volatility;

-- Ditto for players
-- Relax constraints on players
ALTER TABLE player RENAME TO player_old;

CREATE TABLE player (
    id INTEGER PRIMARY KEY,
    short_id CHAR(6) NOT NULL UNIQUE,
    display_name VARCHAR(255) NOT NULL,
    public_key CHAR(64) NOT NULL UNIQUE,
    rating REAL,
    deviation REAL,
    rating_extra TEXT,
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

INSERT INTO player
SELECT
    id, short_id, display_name, public_key,
    rating, deviation,
    CONCAT('(volatility:', CAST(volatility AS TEXT), ')'),
    inserted_at, updated_at
FROM player_old;

DROP TABLE player_old;

-- Migrate participants
ALTER TABLE participant RENAME TO participant_old;

CREATE TABLE participant (
    id INTEGER PRIMARY KEY,
    match_id INTEGER NOT NULL REFERENCES battle(id),
    player_id INTEGER NOT NULL REFERENCES player(id),
    team INTEGER NOT NULL,
    finish_time INTEGER,
    no_contest BOOLEAN NOT NULL DEFAULT FALSE,
    skin VARCHAR(255),
    kart_speed INTEGER,
    kart_weight INTEGER,

    UNIQUE(match_id, player_id)
);

INSERT INTO participant
SELECT *
FROM participant_old;

DROP TABLE participant_old;

ALTER TABLE rating RENAME TO rating_old;

CREATE TABLE rating (
    id INTEGER PRIMARY KEY,
    period_id INTEGER NOT NULL REFERENCES rating_period(id),
    player_id INTEGER NOT NULL REFERENCES player(id),
    rating REAL NOT NULL,
    deviation REAL NOT NULL,
    inserted_at TIMESTAMP NOT NULL,
    extra TEXT NOT NULL,

    UNIQUE(period_id, player_id)
);

INSERT INTO rating
SELECT *
FROM rating_old;

DROP TABLE rating_old;

ALTER TABLE message RENAME TO message_old;

CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    player_id INTEGER NOT NULL REFERENCES player(id),
    content VARCHAR(255) NOT NULL,
    inserted_at TIMESTAMP NOT NULL
);

INSERT INTO message
SELECT *
FROM message_old;

DROP TABLE message_old;
