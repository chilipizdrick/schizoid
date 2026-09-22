# schizoid

A simple discord bot for personal server.

## Features

- Congratulations on members' birthdays (supports custom audio assets via discord slash command attachments)
- Greetings on users joininig voice channel with set time interval (supports custom audio assets via discord slash command attachments)
- Status of set Minecraft server
- Customizable personal role colors
- Persistent data store (SQLite database)

## Setup

Bot expects the following environment variables to be set:

- DATABASE_URL - url of SQLite database to connect to
- DISCORD_TOKEN - self explanatory
- MINECRAFT_SERVER_ADDRESS - address of Minecraft server to check status of (if this feature is enabled in config)
- USER_OWNER_ID - discord user id of bot owner (optional)

User should also provide a custom config in toml format (and provide path to it to the bot via `--config` flag):

```toml
file_storage_path = "./assets"   # Necessary | Path to directory where custom audio assets are stored
max_attachment_size = 10_000_000 # Necessary | Maximum size of attachments in bytes
max_attachment_count = 100       # Necessary | Maximum number of attachments for a discord server (!NOT YET IMPELEMENTED!)

[minecraft_server_ping]
enabled = true           # Optional | Enables checking of Minecraft server status (ping)
ping_timeout_ms = 5000   # Optional | Timeout for pinging Minecraft server (in milliseconds)
ping_interval_ms = 10000 # Optional | Interval for pinging Minecraft server (in milliseconds)
```
