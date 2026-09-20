use std::path::{Path, PathBuf};

use serenity::model::{
    channel::Attachment,
    id::{GuildId, UserId},
};
use songbird::input::Input;
use tokio::{fs, io};

pub struct Storage<'a> {
    root_dir_path: &'a str,
}

impl<'a> Storage<'a> {
    pub fn new(root_dir_path: &'a str) -> Self {
        Self { root_dir_path }
    }

    pub async fn get_member_greeting(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> io::Result<Input> {
        let dir_path = self.member_dir_path(Self::GREETINGS_DIR, guild_id, user_id);
        if !fs::try_exists(&dir_path).await? {
            return self.get_guild_congratulation(guild_id).await;
        }
        let file_path = Self::get_file_path(&dir_path).await?;
        let file = songbird::input::File::new(file_path);
        Ok(file.into())
    }

    pub async fn set_member_greeting(
        &self,
        guild_id: GuildId,
        user_id: UserId,
        file: File<'_>,
    ) -> io::Result<()> {
        let dir_path = self.member_dir_path(Self::GREETINGS_DIR, guild_id, user_id);
        Self::create_file_in_directory(&dir_path, file).await
    }

    /// Returns true, if file existed, and false, if it did not
    pub async fn remove_member_greeting(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> io::Result<RemovalResult> {
        let dir_path = self.member_dir_path(Self::GREETINGS_DIR, guild_id, user_id);
        Self::remove_dir_with_file(dir_path).await
    }

    pub async fn get_guild_greeting(&self, guild_id: GuildId) -> io::Result<Input> {
        let dir_path = self.guild_dir_path(Self::GREETINGS_DIR, guild_id);
        let file_path = Self::get_file_path(&dir_path).await?;
        let file = songbird::input::File::new(file_path);
        Ok(file.into())
    }

    pub async fn set_guild_greeting(&self, guild_id: GuildId, file: File<'_>) -> io::Result<()> {
        let dir_path = self.guild_dir_path(Self::GREETINGS_DIR, guild_id);
        Self::create_file_in_directory(&dir_path, file).await
    }

    pub async fn remove_guild_greeting(&self, guild_id: GuildId) -> io::Result<RemovalResult> {
        let dir_path = self.guild_dir_path(Self::GREETINGS_DIR, guild_id);
        Self::remove_dir_with_file(dir_path).await
    }

    pub async fn get_guild_congratulation(&self, guild_id: GuildId) -> io::Result<Input> {
        let dir_path = self.guild_dir_path(Self::CONGRATULATIONS_DIR, guild_id);
        let file_path = Self::get_file_path(&dir_path).await?;
        let file = songbird::input::File::new(file_path);
        Ok(file.into())
    }

    pub async fn set_guild_congratulation(
        &self,
        guild_id: GuildId,
        file: File<'_>,
    ) -> io::Result<()> {
        let dir_path = self.guild_dir_path(Self::CONGRATULATIONS_DIR, guild_id);
        Self::create_file_in_directory(&dir_path, file).await
    }

    pub async fn remove_guild_congratulation(
        &self,
        guild_id: GuildId,
    ) -> io::Result<RemovalResult> {
        let dir_path = self.guild_dir_path(Self::CONGRATULATIONS_DIR, guild_id);
        Self::remove_dir_with_file(dir_path).await
    }

    const GREETINGS_DIR: &'static str = "greetings";
    const CONGRATULATIONS_DIR: &'static str = "birthday_congratulations";

    fn guild_dir_path(&self, subdir: &str, guild_id: GuildId) -> PathBuf {
        [&self.root_dir_path, subdir, &guild_id.to_string()]
            .iter()
            .collect()
    }

    fn member_dir_path(&self, subdir: &str, guild_id: GuildId, user_id: UserId) -> PathBuf {
        [
            &self.root_dir_path,
            subdir,
            &guild_id.to_string(),
            &user_id.to_string(),
        ]
        .iter()
        .collect()
    }

    /// Writes file to a directory, deleting the previous one, if it existed
    async fn create_file_in_directory(
        dir_path: impl AsRef<Path>,
        file: File<'_>,
    ) -> io::Result<()> {
        if fs::try_exists(&dir_path).await? {
            fs::remove_dir_all(&dir_path).await?;
        }

        fs::create_dir_all(&dir_path).await?;
        let filepath = dir_path.as_ref().join(&file.name);
        fs::write(&filepath, &file.contents).await
    }

    /// Gets and returns path of a file, contained in `subdir`, if it exists
    async fn get_file_path(subdir: impl AsRef<Path>) -> io::Result<PathBuf> {
        let mut read_dir = fs::read_dir(subdir).await?;
        let dir_entry = read_dir
            .next_entry()
            .await?
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(dir_entry.path())
    }

    /// Removed a directory at specified path and returns [`RemovalResult::Existed`] if file existed in that directory,
    /// or returns [`RemovalResult::DidNotExist`]
    async fn remove_dir_with_file(subdir: impl AsRef<Path>) -> io::Result<RemovalResult> {
        let res = Self::get_file_path(subdir.as_ref()).await;
        match res {
            Ok(_) => {
                fs::remove_dir_all(subdir.as_ref()).await?;
                Ok(RemovalResult::Existed)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                fs::remove_dir_all(subdir).await?;
                Ok(RemovalResult::DidNotExist)
            }
            Err(err) => Err(err),
        }
    }
}

#[derive(Debug, Clone)]
pub struct File<'a> {
    name: &'a str,
    contents: Vec<u8>,
}

impl<'a> File<'a> {
    pub fn new(name: &'a str, contents: Vec<u8>) -> Self {
        Self { name, contents }
    }

    pub async fn from_attachment(value: &'a Attachment) -> serenity::Result<Self> {
        let contents = value.download().await?;
        Ok(Self {
            name: &value.filename,
            contents,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RemovalResult {
    Existed,
    DidNotExist,
}
