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
    pub cpu_profiles: Vec<CpuProfile>,
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

#[derive(Clone, Deserialize, Serialize)]
pub struct CpuProfile {
    pub id: String,
    pub label: String,
    pub algorithm: String,
    pub binary: PathBuf,
    pub pools: Vec<Pool>,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CpuSelection {
    pub profile_id: String,
    pub pool_id: String,
    pub wallet: String,
    #[serde(default = "default_cpu_threads_percent")]
    pub threads_percent: u32,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GpuPolicy {
    pub gpu_id: u32,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub throttle_enabled: bool,
    #[serde(default)]
    pub night_mode_enabled: bool,
    #[serde(default)]
    pub external_task_monitor: bool,
    #[serde(default = "default_memory_floor")]
    pub min_free_memory_mib: u32,
    #[serde(default)]
    pub power_limit_w: Option<u32>,
    #[serde(default = "default_intensity")]
    pub intensity: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GpuMode {
    #[default]
    Normal,
    Throttle,
    Night,
}

fn default_memory_floor() -> u32 {
    2048
}

fn default_intensity() -> u32 {
    20
}

fn default_cpu_threads_percent() -> u32 {
    50
}

impl GpuPolicy {
    pub fn new(gpu_id: u32) -> Self {
        Self {
            gpu_id,
            enabled: false,
            throttle_enabled: false,
            night_mode_enabled: false,
            external_task_monitor: false,
            min_free_memory_mib: default_memory_floor(),
            power_limit_w: None,
            intensity: default_intensity(),
        }
    }
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
        let mut cpu_ids = HashSet::new();
        for profile in &self.cpu_profiles {
            if !cpu_ids.insert(&profile.id)
                || profile.id.is_empty()
                || profile.id.len() > 64
                || !profile
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || profile.algorithm.is_empty()
                || profile.algorithm.len() > 64
                || !profile
                    .algorithm
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                bail!("invalid or duplicate CPU profile id or algorithm");
            }
            if profile.binary.is_absolute()
                || profile
                    .binary
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                bail!("CPU binary must be a relative path within gpu_root");
            }
            validate_pools(&profile.pools)?;
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

    pub fn resolve_cpu(&self, s: &CpuSelection) -> Result<(&CpuProfile, &Pool)> {
        validate_wallet(&s.wallet)?;
        if !(1..=100).contains(&s.threads_percent) {
            bail!("threads_percent must be between 1 and 100");
        }
        let profile = self
            .cpu_profiles
            .iter()
            .find(|p| p.id == s.profile_id)
            .context("unknown CPU profile")?;
        let pool = profile
            .pools
            .iter()
            .find(|p| p.id == s.pool_id)
            .context("pool does not belong to CPU profile")?;
        Ok((profile, pool))
    }

    pub fn save_selection(&self, name: &str, selection: &Selection) -> Result<()> {
        self.save_json(name, selection)
    }

    fn save_json<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
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
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
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

    pub fn read_cpu_selection(&self, name: &str) -> Result<CpuSelection> {
        Ok(serde_json::from_slice(&std::fs::read(
            self.state_dir.join(format!("cpu-{name}.json")),
        )?)?)
    }

    pub fn save_cpu_selection(&self, name: &str, selection: &CpuSelection) -> Result<()> {
        self.save_json(&format!("cpu-{name}.json"), selection)
    }

    pub fn read_gpu_policy(&self, gpu_id: u32) -> Result<GpuPolicy> {
        let file = self.state_dir.join(format!("gpu{gpu_id}-policy.json"));
        if !file.exists() {
            return Ok(GpuPolicy::new(gpu_id));
        }

        let policy: GpuPolicy = serde_json::from_slice(&std::fs::read(file)?)?;
        if policy.gpu_id != gpu_id {
            bail!("policy GPU id does not match its file");
        }
        Ok(policy)
    }

    pub fn save_gpu_policy(&self, policy: &GpuPolicy) -> Result<()> {
        self.save_json(&format!("gpu{}-policy.json", policy.gpu_id), policy)
    }

    pub fn read_gpu_mode(&self, gpu_id: u32) -> Result<GpuMode> {
        let file = self.state_dir.join(format!("gpu{gpu_id}-mode.json"));
        if !file.exists() {
            return Ok(GpuMode::Normal);
        }
        Ok(serde_json::from_slice(&std::fs::read(file)?)?)
    }

    pub fn save_gpu_mode(&self, gpu_id: u32, mode: GpuMode) -> Result<()> {
        self.save_json(&format!("gpu{gpu_id}-mode.json"), &mode)
    }
}

fn validate_pools(pools: &[Pool]) -> Result<()> {
    let mut ids = HashSet::new();
    for pool in pools {
        if !ids.insert(&pool.id)
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
    Ok(())
}

fn validate_wallet(wallet: &str) -> Result<()> {
    if wallet.is_empty()
        || wallet.len() > 256
        || wallet.starts_with('-')
        || !wallet
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        bail!("wallet must be 1..256 ASCII letters, digits or ._:- and cannot start with '-'");
    }
    Ok(())
}
