use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

#[derive(Clone, Deserialize)]
pub struct Config {
    pub listen: String,
    pub gpu_root: PathBuf,
    pub state_dir: PathBuf,
    #[serde(default)]
    pub control_enabled: bool,
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub proxy: Option<Proxy>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub miner: String,
    pub binary: PathBuf,
    pub pools: Vec<Pool>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Pool {
    pub id: String,
    pub label: String,
    pub address: String,
    pub tls: bool,
}

#[derive(Clone, Deserialize)]
pub struct Proxy {
    pub endpoint: String,
    pub group: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(default)]
    pub gpu_id: u32,
    pub profile_id: String,
    pub pool_id: String,
    pub wallet: String,
    #[serde(default)]
    pub power_limit_w: Option<u32>,
    pub revision: u64,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let config: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if !self.gpu_root.is_absolute() || !self.state_dir.is_absolute() {
            bail!("gpu_root and state_dir must be absolute paths");
        }
        let mut ids = HashSet::new();
        for p in &self.profiles {
            if !ids.insert(&p.id) || !["srb-pearl", "lol-octopus"].contains(&p.miner.as_str()) {
                bail!("duplicate profile or unsupported miner");
            }
            if p.binary.is_absolute()
                || p.binary
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                bail!("binary must be a relative path within gpu_root");
            }
            let mut pools = HashSet::new();
            for pool in &p.pools {
                if !pools.insert(&pool.id)
                    || pool.address.starts_with('-')
                    || !pool.address.contains(':')
                    || !pool
                        .address
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
                {
                    bail!("invalid pool address or duplicate pool id");
                }
            }
        }
        if let Some(proxy) = &self.proxy {
            let url = reqwest::Url::parse(&proxy.endpoint)?;
            if url.scheme() != "http"
                || !matches!(
                    url.host_str(),
                    Some("127.0.0.1") | Some("localhost") | Some("[::1]")
                )
                || !url.username().is_empty()
                || url.password().is_some()
            {
                bail!("proxy controller must use loopback HTTP without URL credentials");
            }
        }
        Ok(())
    }

    pub fn resolve(&self, s: &Selection) -> Result<(&Profile, &Pool)> {
        if s.gpu_id > 1 {
            bail!("gpu_id must be 0 or 1");
        }
        if s.power_limit_w
            .is_some_and(|power| !(10..=1000).contains(&power))
        {
            bail!("power_limit_w must be between 10 and 1000");
        }
        if s.wallet.is_empty()
            || s.wallet.len() > 256
            || s.wallet.starts_with('-')
            || !s
                .wallet
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        {
            bail!("wallet must be 1..256 ASCII letters, digits or ._:- and cannot start with '-'");
        }
        let profile = self
            .profiles
            .iter()
            .find(|p| p.id == s.profile_id)
            .context("unknown profile")?;
        let pool = profile
            .pools
            .iter()
            .find(|p| p.id == s.pool_id)
            .context("pool does not belong to profile")?;
        Ok((profile, pool))
    }

    pub fn save_selection(&self, name: &str, selection: &Selection) -> Result<()> {
        std::fs::create_dir_all(&self.state_dir)?;
        let temp = self.state_dir.join(format!(".{name}.tmp"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let mut file = options.open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(selection)?)?;
        file.sync_all()?;
        std::fs::rename(temp, self.state_dir.join(name))?;
        Ok(())
    }

    pub fn read_gpu_selection(&self, name: &str, gpu_id: u32) -> Result<Selection> {
        let file = if gpu_id == 0 {
            format!("{name}.json")
        } else {
            format!("gpu{gpu_id}-{name}.json")
        };
        Ok(serde_json::from_slice(&std::fs::read(
            self.state_dir.join(file),
        )?)?)
    }

    pub fn save_gpu_selection(&self, name: &str, selection: &Selection) -> Result<()> {
        let file = if selection.gpu_id == 0 {
            format!("{name}.json")
        } else {
            format!("gpu{}-{name}.json", selection.gpu_id)
        };
        self.save_selection(&file, selection)
    }
}
