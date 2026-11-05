//! Local player management (docs/local-account.md): simple offline profile handling.
//! Accounts are stored directly in `accounts.json` with local UUIDs and names.
//! No Microsoft sign-in, OAuth, or network authentication required.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// What every Play says since there are no Microsoft requirements.
pub const NEEDS_OWNER: &str = "Enter a player name to play";

/// Configuration check always returns None since MSA is disabled.
pub fn client_id() -> Option<String> {
    None
}

/// A mock Endpoints struct to satisfy any remaining structural references.
#[derive(Clone, Debug)]
pub struct Endpoints;

impl Endpoints {
    pub fn live() -> Self {
        Self
    }
}

/// A Minecraft profile, as the launcher shows and remembers it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    /// The profile UUID without dashes.
    pub uuid: String,
    pub name: String,
    /// The active skin's URL and model.
    #[serde(default)]
    pub skin: Option<(String, String)>,
    #[serde(default)]
    pub cape: Option<String>,
}

/// Simplified SecretStore trait or dummy for compatibility.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
    fn persistent(&self) -> bool;
}

#[derive(Default)]
pub struct MemoryStore;
impl SecretStore for MemoryStore {
    fn get(&self, _key: &str) -> Result<Option<String>, String> { Ok(None) }
    fn set(&self, _key: &str, _secret: &str) -> Result<(), String> { Ok(()) }
    fn delete(&self, _key: &str) -> Result<(), String> { Ok(()) }
    fn persistent(&self) -> bool { false }
}

pub fn store() -> &'static dyn SecretStore {
    &MemoryStore
}

pub fn no_keychain() -> Option<String> {
    None
}

// ----- Accounts ------------------------------------------------------------------------------------

/// accounts.json: the local user profiles.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Accounts {
    pub accounts: Vec<Account>,
    pub selected: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offline: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_selected: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub looks: std::collections::BTreeMap<String, crate::skins::Look>,
}

/// Who plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Player {
    Local(String),
    Offline(String),
    GameName,
}

pub fn valid_offline_name(name: &str) -> bool {
    (3..=16).contains(&name.chars().count())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Accounts {
    pub(crate) fn path() -> PathBuf {
        crate::backend::data_dir().join("accounts.json")
    }
    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::path())
    }
    pub(crate) fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub(crate) fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| e.to_string())
    }
    pub fn selected(&self) -> Option<&Account> {
        let uuid = self.selected.as_ref()?;
        self.accounts.iter().find(|a| &a.uuid == uuid)
    }
    pub fn find(&self, who: &str) -> Option<&Account> {
        let undashed = who.replace('-', "").to_lowercase();
        self.accounts
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(who) || a.uuid == undashed)
    }
    
    pub fn add_local(&mut self, name: &str) -> Result<Account, String> {
        let name = name.trim();
        if !valid_offline_name(name) {
            return Err(format!("{name}: a profile name must be 3 to 16 letters, digits or _"));
        }
        // Generate a deterministic or pseudo-random offline UUID for the local profile
        let uuid = format!("{:x}", md5::compute(name.as_bytes())); // Or simple uuid generation
        let account = Account {
            uuid: uuid[..32].to_string(),
            name: name.to_string(),
            skin: None,
            cape: None,
        };
        if !self.accounts.iter().any(|a| a.uuid == account.uuid) {
            self.accounts.push(account.clone());
        }
        self.selected = Some(account.uuid.clone());
        Ok(account)
    }

    pub fn has_owner(&self, _store: &dyn SecretStore) -> bool {
        true // No ownership gate required
    }

    pub fn player(&self) -> Player {
        match (self.selected(), &self.offline_selected) {
            (Some(a), _) => Player::Local(a.uuid.clone()),
            (None, Some(name)) => Player::Offline(name.clone()),
            (None, None) => Player::GameName,
        }
    }

    pub fn play_as(&mut self, _store: &dyn SecretStore, who: Player) -> Result<(), String> {
        match who {
            Player::Local(uuid) => {
                if !self.accounts.iter().any(|a| a.uuid == uuid) {
                    return Err("That profile does not exist.".into());
                }
                self.selected = Some(uuid);
            }
            Player::Offline(name) => {
                let name = self.add_offline(&name)?;
                self.selected = None;
                self.offline_selected = Some(name);
            }
            Player::GameName => {
                self.selected = None;
                self.offline_selected = None;
            }
        }
        Ok(())
    }

    fn add_offline(&mut self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if !valid_offline_name(name) {
            return Err(format!("{name}: an offline name is 3 to 16 letters, digits or _"));
        }
        if let Some(kept) = self.offline.iter().find(|n| n.eq_ignore_ascii_case(name)) {
            return Ok(kept.clone());
        }
        self.offline.push(name.to_string());
        Ok(name.to_string())
    }

    pub fn remove_offline(&mut self, name: &str, skins_dir: &Path) -> Result<(), String> {
        let before = self.offline.len();
        self.offline.retain(|n| !n.eq_ignore_ascii_case(name));
        self.looks.remove(&name.to_ascii_lowercase());
        if self.offline.len() == before {
            return Err(format!("No offline name {name}."));
        }
        crate::skins::forget(skins_dir, name);
        if self.offline_selected.as_ref().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            self.offline_selected = None;
        }
        Ok(())
    }

    pub fn look(&self, name: &str) -> crate::skins::Look {
        self.looks
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    pub fn look_env(&self) -> Vec<(&'static str, String)> {
        match self.name_arg() {
            Some(name) => crate::skins::env(&crate::skins::dir(), name, &self.look(name)),
            None => Vec::new(),
        }
    }

    pub fn name_arg(&self) -> Option<&str> {
        match self.selected() {
            Some(_) => None,
            None => self.offline_selected.as_deref(),
        }
    }

    pub fn remove(&mut self, _store: &dyn SecretStore, uuid: &str) -> Result<(), String> {
        self.accounts.retain(|a| a.uuid != uuid);
        if self.selected.as_deref() == Some(uuid) {
            self.selected = None;
        }
        Ok(())
    }
}

/// Launch struct always succeeds locally without checks.
#[derive(Debug, Default)]
pub struct Launch {
    pub session: Option<(Account, ())>,
    pub offline: Option<String>,
    pub owner: Option<(Account, ())>,
    pub unsaved: Option<String>,
}

pub fn launch(
    _ep: &Endpoints,
    _client_id: Option<&str>,
    _store: &dyn SecretStore,
    accounts: &mut Accounts,
) -> Result<Launch, String> {
    if let Some(account) = accounts.selected().cloned() {
        return Ok(Launch {
            session: Some((account.clone(), ())),
            owner: Some((account, ())),
            ..Default::default()
        });
    }
    Ok(Launch::default())
}

pub fn launch_for_game(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    accounts: &mut Accounts,
    _offline_without_owner: bool,
) -> Result<Launch, String> {
    launch(ep, client_id, store, accounts)
}

/// Write a dummy local session file for the game to consume.
pub fn write_session_file(
    dir: &Path,
    account: &Account,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("session-{}.json", account.uuid));
    let body = json!({
        "name": account.name,
        "uuid": account.uuid,
        "access_token": "offline_token",
        "user_type": "legacy",
        "xuid": "",
    });
    std::fs::write(&path, body.to_string()).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn discard_unread_sessions() {}

#[derive(Debug, Default)]
pub struct GameSession {
    pub file: Option<PathBuf>,
    pub offline: Option<String>,
    pub env: Vec<(&'static str, String)>,
}

pub fn session_for_game(
    game_home: &Path,
    _offline_without_owner: bool,
) -> Result<GameSession, String> {
    let mut accounts = Accounts::load();
    let account = match accounts.selected().cloned() {
        Some(acc) => acc,
        None => accounts.add_local("Player")?,
    };
    let file = write_session_file(&game_home.join("sessions"), &account)?;
    Ok(GameSession {
        file: Some(file),
        ..Default::default()
    })
}

pub fn java_player() -> Result<crate::java_launch::Player, String> {
    let mut accounts = Accounts::load();
    let account = match accounts.selected().cloned() {
        Some(acc) => acc,
        None => accounts.add_local("Player")?,
    };
    Ok(crate::java_launch::Player {
        name: account.name,
        uuid: account.uuid,
        token: "offline_token".to_string(),
        xuid: "".to_string(),
    })
}
