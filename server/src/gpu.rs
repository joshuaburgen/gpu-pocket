use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
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

#[derive(Clone, Debug, Serialize)]
pub struct GpuProcess {
    pub gpu_id: u32,
    pub pid: u32,
    pub process_type: String,
    pub name: String,
    pub memory_used_mib: Option<f64>,
    pub executable: Option<String>,
    pub within_gpu_root: Option<bool>,
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
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            bail!("{program} failed (exit {:?})", result.status.code());
        }
        bail!(
            "{program} failed (exit {:?}): {stderr}",
            result.status.code()
        );
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
        bail!(
            "GPU{gpu_id} has unmanaged PID {pid}; full-intensity takeover requires stopping the old task first"
        );
    }
    Ok(())
}

pub async fn unmanaged_compute_pids(gpu_id: u32) -> Result<Vec<u32>> {
    let gpus = collect().await?;
    let gpu = gpus
        .iter()
        .find(|g| g.index == gpu_id)
        .with_context(|| format!("GPU{gpu_id} not found"))?;
    let processes = parse_compute_processes(
        &output(
            "nvidia-smi",
            &[
                "--query-compute-apps=gpu_uuid,pid,process_name,used_memory",
                "--format=csv,noheader,nounits",
            ],
        )
        .await?,
    )?;
    let expected_service = format!("gpu-pocket-miner@{gpu_id}.service");
    let mut unmanaged = Vec::new();
    for ((uuid, pid), process) in processes {
        if uuid != gpu.uuid || is_ignored_gpu_process(&process.name) {
            continue;
        }
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

pub async fn processes(gpu_root: &Path) -> Result<Vec<GpuProcess>> {
    let gpu_root = gpu_root.canonicalize().context("canonicalize gpu_root")?;
    let mut processes = parse_processes(&output("nvidia-smi", &[]).await?)?;
    let gpu_uuids = parse_gpu_uuids(
        &output(
            "nvidia-smi",
            &["--query-gpu=index,uuid", "--format=csv,noheader,nounits"],
        )
        .await?,
    )?;
    let compute_processes = parse_compute_processes(
        &output(
            "nvidia-smi",
            &[
                "--query-compute-apps=gpu_uuid,pid,process_name,used_memory",
                "--format=csv,noheader,nounits",
            ],
        )
        .await?,
    )?;
    let mut available = Vec::with_capacity(processes.len());
    for mut process in processes.drain(..) {
        if matches!(process.process_type.as_str(), "C" | "C+G") {
            let Some(uuid) = gpu_uuids.get(&process.gpu_id) else {
                bail!("GPU{} process refers to an unknown GPU", process.gpu_id);
            };
            let Some(compute) = compute_processes.get(&(uuid.clone(), process.pid)) else {
                continue;
            };
            let executable = PathBuf::from(&compute.name);
            process.name = executable
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&compute.name)
                .to_string();
            if is_ignored_gpu_process(&process.name) {
                continue;
            }
            process.memory_used_mib = compute.memory_used_mib;
            process.within_gpu_root = Some(executable.starts_with(&gpu_root));
            process.executable = Some(compute.name.clone());
            available.push(process);
            continue;
        }
        let exe_path = PathBuf::from(format!("/proc/{}/exe", process.pid));
        let executable = match std::fs::read_link(&exe_path) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                if !is_ignored_gpu_process(&process.name) {
                    available.push(process);
                }
                continue;
            }
        };
        if is_ignored_gpu_process(
            executable
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
        ) {
            continue;
        }
        process.within_gpu_root = Some(executable.starts_with(&gpu_root));
        process.executable = Some(executable.to_string_lossy().into_owned());
        available.push(process);
    }
    Ok(available)
}

fn parse_gpu_uuids(output: &str) -> Result<BTreeMap<u32, String>> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (index, uuid) = line.split_once(',').context("unrecognized GPU UUID row")?;
            Ok((index.trim().parse()?, uuid.trim().to_string()))
        })
        .collect()
}

#[derive(Debug)]
struct ComputeProcess {
    name: String,
    memory_used_mib: Option<f64>,
}

fn parse_compute_processes(output: &str) -> Result<BTreeMap<(String, u32), ComputeProcess>> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.contains("No running processes"))
        .map(|line| {
            let fields: Vec<_> = line.split(',').map(str::trim).collect();
            if fields.len() != 4 {
                bail!("unrecognized compute process row");
            }
            let pid = fields[1].parse().context("invalid compute process id")?;
            let memory_used_mib = fields[3].parse::<f64>().ok();
            Ok((
                (fields[0].to_string(), pid),
                ComputeProcess {
                    name: fields[2].to_string(),
                    memory_used_mib,
                },
            ))
        })
        .collect()
}

fn parse_processes(output: &str) -> Result<Vec<GpuProcess>> {
    let section = output
        .split_once("Processes:")
        .map(|(_, section)| section)
        .context("nvidia-smi output does not contain a Processes section")?;
    let mut processes = Vec::new();
    for line in section.lines() {
        let line = line.trim().trim_matches('|').trim();
        if line.is_empty()
            || line.contains("Process name")
            || line.starts_with("GPU ")
            || line.chars().all(|character| character == '=')
            || line.starts_with("No running processes")
        {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 7 || !matches!(fields[4], "C" | "G" | "C+G") {
            continue;
        }
        let Ok(gpu_id) = fields[0].parse() else {
            continue;
        };
        let pid: u32 = fields[3].parse().context("invalid GPU process id")?;
        let name = fields[5..fields.len() - 1].join(" ");
        if name.is_empty() || is_xorg(&name) {
            continue;
        }
        let memory_used_mib = fields
            .last()
            .and_then(|value| value.strip_suffix("MiB"))
            .and_then(|value| value.parse::<f64>().ok());
        processes.push(GpuProcess {
            gpu_id,
            pid,
            process_type: fields[4].to_string(),
            name,
            memory_used_mib,
            executable: None,
            within_gpu_root: None,
        });
    }
    Ok(processes)
}

fn is_xorg(name: &str) -> bool {
    Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("xorg"))
}

fn is_ignored_gpu_process(name: &str) -> bool {
    let name = Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(name)
        .to_ascii_lowercase();
    ["sunlogin", "chrome", "chromium", "google-chrome", "cursor"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

pub fn demo() -> Vec<Gpu> {
    parse("0, GPU-demo-0, NVIDIA GeForce RTX 3080 Ti (DEMO), 92, 540, 12288, 65, 280, 350, 100, 400, 70, P2\n1, GPU-demo-1, NVIDIA GeForce RTX 3080 Ti (DEMO), 80, 5492, 12288, 68, 145, 350, 100, 400, N/A, P2").unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gpu_processes_and_omits_xorg() {
        let output = r#"
Processes:
|  GPU   GI   CI        PID   Type   Process name                          GPU Memory |
|=====================================================================================|
|    0   N/A  N/A    123456      C   /media/new_data3/Tongji-Wang/gpu/worker  512MiB |
|    1   N/A  N/A    234567      G   /usr/lib/xorg/Xorg                      128MiB |
|    1   N/A  N/A    345678      C   python3                                 1024MiB |
"#;
        let processes = parse_processes(output).unwrap();
        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0].gpu_id, 0);
        assert_eq!(processes[0].pid, 123456);
        assert_eq!(processes[0].memory_used_mib, Some(512.0));
        assert_eq!(processes[1].gpu_id, 1);
        assert_eq!(processes[1].name, "python3");
    }

    #[test]
    fn resolves_compute_process_paths_from_nvidia_query() {
        let gpus = parse_gpu_uuids("0, GPU-a\n1, GPU-b\n").unwrap();
        let processes =
            parse_compute_processes("GPU-b, 345678, /media/Tongji-Wang/gpu/miner, 1024\n").unwrap();
        let process = processes.get(&(gpus[&1].clone(), 345678)).unwrap();
        assert_eq!(process.name, "/media/Tongji-Wang/gpu/miner");
        assert_eq!(process.memory_used_mib, Some(1024.0));
    }

    #[test]
    fn ignores_requested_background_applications_only() {
        for name in [
            "/usr/bin/SunloginClient",
            "/usr/bin/google-chrome-stable",
            "/usr/lib/chromium/chromium",
            "/opt/Cursor/cursor",
        ] {
            assert!(is_ignored_gpu_process(name), "{name}");
        }
        for name in ["/usr/bin/python3", "/opt/miner/custom-worker"] {
            assert!(!is_ignored_gpu_process(name), "{name}");
        }
    }
}
