-- A list of users on the duelchannel
CREATE TABLE user (
    id INTEGER PRIMARY KEY,
    username VARCHAR(255) NOT NULL UNIQUE,
    -- The monoys
    mobiums BIGINT NOT NULL DEFAULT 400,
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

-- A list of RR profiles
CREATE TABLE player (
    id INTEGER PRIMARY KEY,
    display_name VARCHAR(255) NOT NULL,
    public_key CHAR(64) NOT NULL UNIQUE,
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

-- A list of matches on the duelchannel
CREATE TABLE battle (
    id INTEGER PRIMARY KEY,
    uuid CHAR(36) NOT NULL UNIQUE,
    level_name VARCHAR(255) NOT NULL,
    -- Whether bets are accepted right now
    accepting_bets BOOLEAN NOT NULL,
    -- The victor of the match
    -- 0 for red, 1 for blue
    victor INTEGER,
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE TABLE participant (
    match_id INTEGER NOT NULL REFERENCES battle(id),
    player_id INTEGER NOT NULL REFERENCES player(id),
    team INTEGER NOT NULL,

    UNIQUE (match_id, player_id)
);

-- A list of bets
CREATE TABLE wager (
    id INTEGER PRIMARY KEY,
    -- Who made the bet
    user_id INTEGER NOT NULL REFERENCES user(id),
    -- On what match was the bet made
    match_id INTEGER NOT NULL REFERENCES match(id),
    -- The victor of the match
    -- 0 for red, 1 for blue
    victor INTEGER NOT NULL,
    -- How many monoys are on the bet
    mobiums BIGINT NOT NULL,
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,

    -- A user can only have one bet on each match
    UNIQUE (user_id, match_id)
);
