CREATE TABLE guilds (
    id VARCHAR PRIMARY KEY NOT NULL, -- discord ID
    greeting_enabled BOOLEAN NOT NULL,
    birthday_enabled BOOLEAN NOT NULL,
    greeting_interval INTEGER NOT NULL,
    timezone TEXT NOT NULL
);

CREATE TABLE users (
    id VARCHAR PRIMARY KEY NOT NULL, -- discord ID
    -- is_admin BOOLEAN NOT NULL,
    birthday_day INTEGER,
    birthday_month INTEGER
);

CREATE TABLE members (
    user_id VARCHAR NOT NULL, -- discord ID
    guild_id VARCHAR NOT NULL, -- discord ID
    greeting_enabled BOOLEAN NOT NULL,
    last_greeting DATETIME, -- ISO 8601 datetime
    last_birthday_congratulation DATETIME, -- ISO 8601 datetime
    PRIMARY KEY (user_id, guild_id)
);
