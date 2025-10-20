-- A list of users on the duelchannel
CREATE TABLE user (
    id INTEGER PRIMARY KEY,
    username VARCHAR(255) NOT NULL UNIQUE,
    -- The monoys
    mobiums BIGINT NOT NULL DEFAULT 400,
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

-- A list of matches on the duelchannel
CREATE TABLE match (
    id INTEGER PRIMARY KEY,
    level_name VARCHAR(255) NOT NULL,
    -- The outcome i.e. the winner of the match
    -- May be null if the match isn't completed
    victor CHAR(64),
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

-- A list of bets
CREATE TABLE bet (
    id INTEGER PRIMARY KEY,
    -- Who made the bet
    user_id INTEGER NOT NULL REFERENCES user(id),
    -- On what match was the bet made
    match_id INTEGER NOT NULL REFERENCES match(id),
    -- The predicted outcome of the bet
    victor CHAR(64) NOT NULL,
    -- How many monoys are on the bet
    mobiums BIGINT NOT NULL,
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,

    -- A user can only have one bet on each match
    UNIQUE (user_id, match_id)
);

-- A list of RR profiles
CREATE TABLE profile (
    id INTEGER PRIMARY KEY,
    display_name VARCHAR(255) NOT NULL,
    public_key CHAR(64) NOT NULL UNIQUE
)
