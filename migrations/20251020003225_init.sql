-- A list of users on the duelchannel
CREATE TABLE user (
    id INTEGER PRIMARY KEY,
    username VARCHAR(255) NOT NULL UNIQUE,
    -- The monoys
    mobiums BIGINT NOT NULL DEFAULT 400,
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

-- A list of Discord authentications
CREATE TABLE discord_auth (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL UNIQUE REFERENCES user(id),
    discord_id BIGINT NOT NULL UNIQUE,
    refresh_token VARCHAR(255) NOT NULL,
    inserted_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

-- A list of RR profiles
CREATE TABLE player (
    id INTEGER PRIMARY KEY,
    short_id CHAR(6) NOT NULL UNIQUE,
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
    -- If the match is completed
    concluded BOOLEAN NOT NULL DEFAULT FALSE,
    -- May be NULL if the match isn't over.
    concluded_at TIMESTAMP,
    -- The time of wagers closing for this match
    closed_at TIMESTAMP NOT NULL,
    inserted_at TIMESTAMP NOT NULL
);

CREATE TABLE participant (
    id INTEGER PRIMARY KEY,
    match_id INTEGER NOT NULL REFERENCES battle(id),
    player_id INTEGER NOT NULL REFERENCES player(id),
    team INTEGER NOT NULL,
    -- The finishing time of the participant
    finish_time INTEGER,
    -- Whether the participant no contest'd
    no_contest BOOLEAN NOT NULL DEFAULT FALSE,

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
