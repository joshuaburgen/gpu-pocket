use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Serialize)]
pub struct Gpu {
    pub index: u32,
    pub uuid: String,
    pub name: String,
    pub utilization: Option<f64>,
    pub memory_used_mib: Option<f64>,
    pub memory_total_mib: Option<f64>,
    pub temperature_c: Option<f64>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
    pub power_min_limit_w: Option<f64>,
    pub power_max_limit_w: Option<f64>,
    pub fan_percent: Option<f64>,
}

pub async fn output(program: &str, args: &[&str]) -> Result<String> {
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("command timed out")??;
    if !result.status.success() {
        bail!("{program} failed (exit {:?})", result.status.code());
    }
    Ok(String::from_utf8(result.stdout)?)
}

pub fn parse(csv: &str) -> Result<Vec<Gpu>> {
    csv.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let c: Vec<_> = line.split(',').map(str::trim).collect();
            if c.len() != 13 {
                bail!("unexpected nvidia-smi field count");
            }
            let number = |i: usize| c[i].parse::<f64>().ok().filter(|x| x.is_finite());
            Ok(Gpu {
                index: c[0].parse()?,
                uuid: c[1].into(),
                name: c[2].into(),
                utilization: number(3),
                memory_used_mib: number(4),
                memory_total_mib: number(5),
                temperature_c: number(6),
                power_w: number(7),
                power_limit_w: number(8),
                power_min_limit_w: number(9),
                power_max_limit_w: number(10),
                fan_percent: number(11),
            })
        })
        .collect()
}

pub async fn collect() -> Result<Vec<Gpu>> {
    // Keep unavailable numeric readings as null.
    parse(&output("nvidia-smi", &["--query-gpu=index,uuid,name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,power.limit,power.min_limit,power.max_limit,fan.speed,pstate", "--format=csv,noheader,nounits"]).await?)
}

pub async fn ensure_gpu_owned(gpu_id: u32) -> Result<()> {
    let unmanaged = unmanaged_compute_pids(gpu_id).await?;
    if let Some(pid) = unmanaged.first() {
        bail!("GPU{gpu_id} is occupied by unmanaged PID {pid}; stop the old supervisor manually before takeover");
    }
    Ok(())
}

pub async fn unmanaged_compute_pids(gpu_id: u32) -> Result<Vec<u32>> {
    let gpus = collect().await?;
    let gpu = gpus
        .iter()
        .find(|g| g.index == gpu_id)
        .with_context(|| format!("GPU{gpu_id} not found"))?;
    let processes = output(
        "nvidia-smi",
        &[
            "--query-compute-apps=gpu_uuid,pid",
            "--format=csv,noheader,nounits",
        ],
    )
    .await?;
    let expected_service = format!("gpu-pocket-miner@{gpu_id}.service");
    let mut unmanaged = Vec::new();
    for line in processes.lines() {
        if line.trim().is_empty() || line.contains("No running processes") {
            continue;
        }
        let Some((uuid, pid)) = line.split_once(',') else {
            bail!("unrecognized GPU process row");
        };
        if uuid.trim() != gpu.uuid {
            continue;
        }
        let pid: u32 = pid.trim().parse().context("invalid process id")?;
        let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).with_context(|| {
            format!("cannot verify GPU{gpu_id} process ownership; retry after process exits")
        })?;
        if !cgroup.lines().any(|line| {
            line.split(':')
                .next_back()
                .unwrap_or("")
                .split('/')
                .any(|c| c == expected_service)
        }) {
            unmanaged.push(pid);
        }
    }
    Ok(unmanaged)
}

pub fn demo() -> Vec<Gpu> {
    parse("0, GPU-demo-0, NVIDIA GeForce RTX 3080 Ti (DEMO), 92, 540, 12288, 65, 280, 350, 100, 400, 70, P2\n1, GPU-demo-1, NVIDIA GeForce RTX 3080 Ti (DEMO), 80, 5492, 12288, 68, 145, 350, 100, 400, N/A, P2").unwrap()
}
