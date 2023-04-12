use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use clap::Parser;
use directories::ProjectDirs;
use posix_cli_utils::IoContext;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};

const DEFAULT_CONFIG_TOML: &str = include_str!("../default-config.toml");

pub fn deserialize_string_lowercase<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let mut s = String::deserialize(deserializer)?;
    s.make_ascii_lowercase();
    Ok(s)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plant {
    pub watering_interval: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub plants: HashMap<String, Plant>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlantStatus {
    pub last_watered: NaiveDateTime,
}

impl Default for PlantStatus {
    fn default() -> Self {
        Self {
            last_watered: NaiveDateTime::new(
                NaiveDate::from_ymd_opt(1900, 1, 1).unwrap(),
                NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
            ),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub plants: HashMap<String, PlantStatus>,
}

fn state_path(dirs: &ProjectDirs) -> PathBuf {
    dirs.config_dir().join("state.toml")
}

fn config_path(dirs: &ProjectDirs) -> PathBuf {
    dirs.config_dir().join("config.toml")
}

fn write_toml<T: Serialize, P: AsRef<Path>>(val: T, path: P) -> Result<()> {
    let contents = toml::to_string_pretty(&val)?;
    let path = path.as_ref();
    std::fs::write(path, contents).context_write(path)
}

fn read_toml<T: DeserializeOwned, P: AsRef<Path>>(path: P) -> Result<T> {
    let path = path.as_ref();
    let contents = std::fs::read_to_string(path).context_read(path)?;
    toml::from_str(&contents).context("failed to deserialise")
}

fn load_config(dirs: &ProjectDirs) -> Result<Config> {
    let path = config_path(dirs);
    if path.exists() {
        read_toml(path)
    } else {
        println!("no config exists, create config at {}", path.display());
        std::fs::write(&path, DEFAULT_CONFIG_TOML).context_write(&path)?;
        Ok(toml::from_str(DEFAULT_CONFIG_TOML).unwrap())
    }
}

fn load_state(dirs: &ProjectDirs) -> Result<State> {
    let path = state_path(dirs);
    if path.exists() {
        read_toml(path)
    } else {
        Ok(State::default())
    }
}

fn write_state(dirs: &ProjectDirs, state: &State) -> Result<()> {
    let path = state_path(dirs);
    write_toml(state, path)
}

fn sync_state_with_config(config: &Config, state: &mut State) {
    state
        .plants
        .retain(|plant, _| config.plants.contains_key(plant));
    for plant in config.plants.keys() {
        if !state.plants.contains_key(&**plant) {
            state.plants.insert(plant.clone(), PlantStatus::default());
        }
    }
}

fn cmd_water(dirs: &ProjectDirs, args: WaterArgs) -> Result<()> {
    let config = load_config(dirs)?;
    let mut state = load_state(dirs)?;
    sync_state_with_config(&config, &mut state);
    let now = chrono::Local::now().naive_local();
    if args.all {
        for (name, plant) in &config.plants {
            let status = state.plants.get_mut(name).unwrap();
            if (now - status.last_watered).num_days() >= plant.watering_interval as i64 {
                status.last_watered = now;
            }
        }
    } else {
        for plant in &args.plants {
            if !config.plants.contains_key(&**plant) {
                bail!("no plant named {plant} in config")
            }
        }
        for plant in &args.plants {
            state.plants.get_mut(plant).unwrap().last_watered = now;
        }
    };

    write_state(dirs, &state)
}

fn cmd_nag(dirs: &ProjectDirs) -> Result<()> {
    let now = chrono::Local::now().naive_local();
    let mut state = load_state(dirs)?;
    let config = load_config(dirs)?;
    sync_state_with_config(&config, &mut state);
    for (plant, status) in state.plants {
        let days = (now - status.last_watered).num_days();
        let &Plant {
            watering_interval: watering_frequency,
        } = config.plants.get(&plant).unwrap();
        if watering_frequency as i64 <= days {
            println!(
                "Plant needs watering: {} ({} days since last watered)",
                &plant, days
            );
        }
    }
    Ok(())
}

#[derive(Parser)]
struct WaterArgs {
    /// plant names
    plants: Vec<String>,
    /// mark all plants as being watered, which needed to be watered.
    #[clap(short = 'a')]
    all: bool,
}

#[derive(Parser)]
enum Command {
    /// nags you about unwatered houseplants
    Nag,
    /// marks plants as being watered
    Water(WaterArgs),
}

fn main() -> Result<()> {
    let cmd = Command::parse();
    let dirs = directories::ProjectDirs::from("", "", "plant-paladin")
        .ok_or_else(|| anyhow!("unable to retrieve user home dir"))?;
    if !dirs.config_dir().exists() {
        std::fs::create_dir(dirs.config_dir())?;
    }
    match cmd {
        Command::Nag => cmd_nag(&dirs),
        Command::Water(args) => cmd_water(&dirs, args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_parses() -> Result<()> {
        let _: Config = toml::from_str(DEFAULT_CONFIG_TOML)?;
        Ok(())
    }
}
