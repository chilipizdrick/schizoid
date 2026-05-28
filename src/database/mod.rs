use std::str::FromStr;

use anyhow::anyhow;
use chrono::{DateTime, Datelike, NaiveDateTime, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use serenity::{
    all::{GuildId, UserId},
    prelude::TypeMapKey,
};
use sqlx::{SqlitePool, query, query_as};

pub struct DBConnKey;

impl TypeMapKey for DBConnKey {
    type Value = DatabaseConnection;
}

pub struct DatabaseConnection {
    pub pool: SqlitePool,
}

impl DatabaseConnection {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_user(&self, user_id: UserId) -> anyhow::Result<Option<User>> {
        let id_str = user_id.to_string();
        let user_row = query_as!(UserRow, "SELECT * FROM users WHERE id = ?", id_str)
            .fetch_optional(&self.pool)
            .await?;
        let user = user_row.map(|ur| ur.try_into()).transpose()?;
        Ok(user)
    }

    pub async fn get_guild(&self, guild_id: GuildId) -> anyhow::Result<Option<Guild>> {
        let id_str = guild_id.to_string();
        let guild_row = query_as!(GuildRow, "SELECT * FROM guilds WHERE id = ?", id_str)
            .fetch_optional(&self.pool)
            .await?;
        let guild = guild_row.map(|gr| gr.try_into()).transpose()?;
        Ok(guild)
    }

    pub async fn get_member(
        &self,
        user_id: UserId,
        guild_id: GuildId,
    ) -> anyhow::Result<Option<Member>> {
        let user_id = user_id.to_string();
        let guild_id = guild_id.to_string();
        let member_row = query_as!(
            MemberRow,
            "SELECT * FROM members WHERE user_id = ? AND guild_id = ?",
            user_id,
            guild_id
        )
        .fetch_optional(&self.pool)
        .await?;
        let member = member_row.map(|gur| gur.try_into()).transpose()?;
        Ok(member)
    }

    pub async fn get_or_insert_user(&self, user_id: UserId) -> anyhow::Result<User> {
        let user = self.get_user(user_id).await?;

        match user {
            Some(user) => Ok(user),
            None => {
                let user = User::default_with_id(user_id);
                let user_row = user.clone().into();
                let UserRow {
                    id,
                    birthday_month,
                    birthday_day,
                } = user_row;

                query!(
                    "INSERT INTO users (id, birthday_month, birthday_day) VALUES ( ?, ?, ? )",
                    id,
                    birthday_month,
                    birthday_day
                )
                .execute(&self.pool)
                .await?;

                Ok(user)
            }
        }
    }

    pub async fn get_or_insert_guild(&self, guild_id: GuildId) -> anyhow::Result<Guild> {
        let guild = self.get_guild(guild_id).await?;

        match guild {
            Some(user) => Ok(user),
            None => {
                let guild = Guild::default_with_id(guild_id);
                let guild_row = guild.clone().into();
                let GuildRow {
                    id,
                    greeting_enabled,
                    birthday_enabled,
                    greeting_interval,
                    timezone,
                } = guild_row;

                query!(
                    r#"INSERT INTO guilds (id, greeting_enabled, birthday_enabled, greeting_interval, timezone)
                      VALUES ( ?, ?, ?, ?, ? )"#,
                    id,
                    greeting_enabled,
                    birthday_enabled,
                    greeting_interval,
                    timezone
                )
                .execute(&self.pool)
                .await?;

                Ok(guild)
            }
        }
    }

    pub async fn get_or_insert_member(
        &self,
        user_id: UserId,
        guild_id: GuildId,
    ) -> anyhow::Result<Member> {
        let member = self.get_member(user_id, guild_id).await?;

        match member {
            Some(member) => Ok(member),
            None => {
                let member = Member::default_with_ids(user_id, guild_id);
                let member_row = member.clone().into();
                let MemberRow {
                    user_id,
                    guild_id,
                    greeting_enabled,
                    last_greeting,
                    last_birthday_congratulation,
                } = member_row;

                query!(
                    r#"INSERT INTO members (user_id, guild_id, greeting_enabled, last_greeting, last_birthday_congratulation)
                    VALUES ( ?, ?, ?, ?, ? )"#,
                    user_id,
                    guild_id,
                    greeting_enabled,
                    last_greeting,
                    last_birthday_congratulation
                )
                .execute(&self.pool)
                .await?;

                Ok(member)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthDayDate {
    pub month: u32,
    pub day: u32,
}

impl MonthDayDate {
    fn new(month: u32, day: u32) -> anyhow::Result<Self> {
        if !(1..=12).contains(&month) {
            return Err(anyhow!("Month must be between 1 and 12"));
        }

        if !(1..=31).contains(&day) {
            return Err(anyhow!("Day must be between 1 and 31"));
        }

        Ok(Self { month, day })
    }
}

impl FromStr for MonthDayDate {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut month_day = s.trim().split('/');
        let month_str = month_day
            .next()
            .ok_or_else(|| anyhow!("Error parsing month of MM/DD"))?;
        let day_str = month_day
            .next()
            .ok_or_else(|| anyhow!("Error parsing day of MM/DD"))?;

        let month = month_str.parse()?;
        let day = day_str.parse()?;

        Ok(Self { month, day })
    }
}

impl<Tz> From<DateTime<Tz>> for MonthDayDate
where
    Tz: TimeZone,
{
    fn from(value: DateTime<Tz>) -> Self {
        let month = value.month();
        let day = value.day();
        Self { month, day }
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub birthday: Option<MonthDayDate>,
}

impl User {
    pub fn default_with_id(id: UserId) -> Self {
        Self { id, birthday: None }
    }
}

impl TryFrom<UserRow> for User {
    type Error = anyhow::Error;

    fn try_from(value: UserRow) -> Result<Self, Self::Error> {
        let UserRow {
            id,
            birthday_day,
            birthday_month,
        } = value;

        let id = id.parse()?;

        let birthday = match (birthday_month, birthday_day) {
            (Some(month), Some(day)) => Some(MonthDayDate::new(month as u32, day as u32)?),
            _ => None,
        };

        Ok(Self { id, birthday })
    }
}

#[derive(Debug, Clone)]
pub struct UserRow {
    id: String,
    birthday_month: Option<i64>,
    birthday_day: Option<i64>,
}

impl From<User> for UserRow {
    fn from(value: User) -> Self {
        let User { id, birthday } = value;

        let id = id.to_string();
        let birthday_day = birthday.map(|b| b.day as i64);
        let birthday_month = birthday.map(|b| b.month as i64);

        Self {
            id,
            birthday_month,
            birthday_day,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Guild {
    pub id: GuildId,
    pub greeting_enabled: bool,
    pub birthday_enabled: bool,
    pub greeting_interval: TimeDelta,
    pub timezone: Tz,
}

impl Guild {
    pub fn default_with_id(id: GuildId) -> Self {
        Self {
            id,
            greeting_enabled: false,
            birthday_enabled: true,
            greeting_interval: TimeDelta::days(1),
            timezone: Tz::UTC,
        }
    }
}

impl TryFrom<GuildRow> for Guild {
    type Error = anyhow::Error;

    fn try_from(value: GuildRow) -> Result<Self, Self::Error> {
        let GuildRow {
            id,
            greeting_enabled,
            birthday_enabled,
            greeting_interval,
            timezone,
        } = value;

        let id = id.parse()?;
        let timezone = timezone.parse()?;
        let greeting_interval = TimeDelta::seconds(greeting_interval);

        Ok(Self {
            id,
            greeting_enabled,
            birthday_enabled,
            greeting_interval,
            timezone,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GuildRow {
    id: String,
    greeting_enabled: bool,
    birthday_enabled: bool,
    greeting_interval: i64,
    timezone: String,
}

impl From<Guild> for GuildRow {
    fn from(value: Guild) -> Self {
        let Guild {
            id,
            greeting_enabled,
            birthday_enabled,
            greeting_interval,
            timezone,
        } = value;

        let id = id.to_string();
        let timezone = timezone.to_string();
        let greeting_interval = greeting_interval.num_seconds();

        Self {
            id,
            greeting_enabled,
            birthday_enabled,
            greeting_interval,
            timezone,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Member {
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub greeting_enabled: bool,
    pub last_greeting: Option<DateTime<Utc>>,
    pub last_birthday_congratulation: Option<DateTime<Utc>>,
}

impl Member {
    fn default_with_ids(user_id: UserId, guild_id: GuildId) -> Self {
        Self {
            user_id,
            guild_id,
            greeting_enabled: true,
            last_greeting: None,
            last_birthday_congratulation: None,
        }
    }
}

impl TryFrom<MemberRow> for Member {
    type Error = anyhow::Error;

    fn try_from(value: MemberRow) -> Result<Self, Self::Error> {
        let MemberRow {
            user_id,
            guild_id,
            greeting_enabled,
            last_greeting,
            last_birthday_congratulation,
        } = value;

        let user_id = user_id.parse()?;
        let guild_id = guild_id.parse()?;
        let last_greeting = last_greeting.map(|dt| dt.and_utc());
        let last_birthday_congratulation = last_birthday_congratulation.map(|dt| dt.and_utc());

        Ok(Self {
            user_id,
            guild_id,
            greeting_enabled,
            last_greeting,
            last_birthday_congratulation,
        })
    }
}

#[derive(Debug, Clone)]
pub struct MemberRow {
    user_id: String,
    guild_id: String,
    greeting_enabled: bool,
    last_greeting: Option<NaiveDateTime>,
    last_birthday_congratulation: Option<NaiveDateTime>,
}

impl From<Member> for MemberRow {
    fn from(value: Member) -> Self {
        let Member {
            user_id,
            guild_id,
            greeting_enabled,
            last_greeting,
            last_birthday_congratulation,
        } = value;

        let user_id = user_id.to_string();
        let guild_id = guild_id.to_string();
        let last_greeting = last_greeting.map(|dt| dt.naive_utc());
        let last_birthday_congratulation = last_birthday_congratulation.map(|dt| dt.naive_utc());

        Self {
            user_id,
            guild_id,
            greeting_enabled,
            last_greeting,
            last_birthday_congratulation,
        }
    }
}
