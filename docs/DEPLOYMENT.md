# 部署与逐 GPU 接管

更新：生产环境使用用户级 API 与 Quick Tunnel；GPU0/GPU1 已由独立的 `gpu-pocket-miner@<id>.service` 管理。本轮 SRBMiner → Krig 1.5.12 的切换因服务器 glibc 版本不兼容而暂停；生产配置、二进制和运行中矿工未更改。日常使用参见 [当前连接与运维](QUICK_TUNNEL.md)。不要在已有用户级 API 占用 8787 时重复安装或启动系统级 API。

## 部署前状态

基础开发没有修改原有矿工脚本、停止现有工作、安装 root 服务或公开 API。当前 SSH 用户为 `joshua`，`sudo -n true` 提示需要密码。以下 root 安装步骤由机器管理员在可信终端执行。先验证只读监控，再逐卡决定是否接管 GPU0、GPU1。

## 1. 只读 API

在 Linux 上构建并安装：

```bash
cargo test --manifest-path server/Cargo.toml
cargo build --release --manifest-path server/Cargo.toml
command -v ip runuser systemctl
sudo install -d -m 0755 /opt/gpu-pocket
sudo install -d -m 0750 -o root -g joshua /etc/gpu-pocket
sudo install -d -m 0700 -o joshua -g joshua /var/lib/gpu-pocket
sudo install -m 0755 server/target/release/gpu-pocket-server /opt/gpu-pocket/gpu-pocket-server
sudo install -m 0640 -o root -g joshua server/config.example.toml /etc/gpu-pocket/server.toml
sudo install -m 0644 deploy/gpu-pocket-api.service /etc/systemd/system/
sudoedit /etc/gpu-pocket/api.env
```

当前机器已验证 `/usr/sbin/ip`、`/usr/sbin/runuser`、`/usr/bin/systemctl`。更换发行版时应同步调整 unit 与 sudoers 路径。

在可信终端运行 `python3 -c 'import secrets; print(secrets.token_urlsafe(32))'` 生成新令牌。`api.env` 填写：

```ini
GPU_POCKET_TOKEN=替换为至少32字符的随机ASCII令牌
# 接入专用 Mihomo 控制器时才需要：
# GPU_POCKET_PROXY_SECRET=专用控制器秘密
```

```bash
sudo chmod 0600 /etc/gpu-pocket/api.env
sudo systemctl daemon-reload
sudo systemctl enable --now gpu-pocket-api.service
sudo journalctl -u gpu-pocket-api.service -n 30 --no-pager
```

此时保持 `control_enabled = false`。默认只监听 `127.0.0.1:8787`。监控两张卡不需要 sudo。开发机可用 `ssh -N -L 8787:127.0.0.1:8787 2-titan` 访问远端回环接口。未带令牌应返回 401；正确令牌请求 `/api/v1/status` 应显示 `demo: false` 与真实数据。

### Firebase 手机推送（可选）

在 Firebase Console 创建项目并登记 Android 应用，包名为 `com.tongjiwang.gpu_pocket`。从项目设置取得 `project_id`、`messagingSenderId`、Web API key 和 Android App ID。构建手机端时，将它们作为 `GPU_POCKET_FCM_PROJECT_ID`、`GPU_POCKET_FCM_SENDER_ID`、`GPU_POCKET_FCM_API_KEY`、`GPU_POCKET_FCM_APP_ID` 的 Dart define 提供（见 [Android 构建文档](ANDROID.md)）。这些客户端参数不是服务账号私钥。

在 Firebase 启用 Cloud Messaging API，并创建具备 Firebase Cloud Messaging API 发送权限的服务账号。将下载的 JSON 通过可信管理通道放到服务器的 `/etc/gpu-pocket/`，不可放进仓库、App 或普通日志。设置所有者和权限后，在 `/etc/gpu-pocket/api.env` 添加：

```ini
GPU_POCKET_FCM_CREDENTIALS=/etc/gpu-pocket/firebase-service-account.json
```

```bash
sudo chown root:joshua /etc/gpu-pocket/firebase-service-account.json
sudo chmod 0640 /etc/gpu-pocket/firebase-service-account.json
sudo chmod 0600 /etc/gpu-pocket/api.env
sudo systemctl restart gpu-pocket-api.service
```

设备连接 API 后会申请通知权限并注册 FCM 令牌。`/var/lib/gpu-pocket/push-tokens.json` 由服务以权限 `0600` 保存；用户删除 App 连接时，客户端会尝试注销自己的令牌。状态中的 `push_configured` 仅表示服务账号文件存在，不代表 Google 已验证凭据或推送已成功；需查看 API 日志并执行实际手机通知验收。未配置 Firebase 时，进程列表仍可查看，但推送无法投递。

若服务器直连 Google 不通，可在 API 服务环境中单独设置 `GPU_POCKET_FCM_PROXY`（例如 `http://127.0.0.1:7890`）。此代理只用于 OAuth/FCM 出站请求，不影响手机访问 API 的连接；先验证代理能建立到 `oauth2.googleapis.com` 和 `fcm.googleapis.com` 的 HTTPS 连接，再重启服务并实际验收通知。

服务端进程监视器每 5 秒采样 GPU 进程并排除 Xorg、Sunlogin、Chrome/Chromium 和 Cursor。其余 GPU 进程仍按是否位于 `gpu_root` 外识别，不因进程所有者而过滤；不属于对应 GPU Pocket 服务的 compute 进程也会阻止接管。新出现的目录外进程会发送推送；相同进程持续运行不会重复通知，退出后再出现会再次通知。服务启动后的首次采样作为基线，不会把启动前已有的进程当成新进程告警。

手机设置页的矿池候选来自服务端配置的白名单，而不是客户端硬编码。新增或调整节点时，更新 `/etc/gpu-pocket/server.toml` 中对应的 profile/pool 条目并重启 API；`server/config.example.toml` 提供 Kryptex PRL/XMR 与 NiceHash OCTOPUS 的区域节点样例。服务端仍会拒绝未配置的节点。

## 2. HTTPS 入口

先修复主机 DNS，再按 [Cloudflare 官方文档](https://developers.cloudflare.com/tunnel/get-started/) 安装 cloudflared、创建命名隧道与域名。参考 `deploy/cloudflared.example.yml`，将域名仅映射到 `http://127.0.0.1:8787`。样例采用 HTTP/2；本次仅验证了 TCP 7844，未验证 QUIC。无需给服务器开放 API 8787 入站。

基础客户端没有集成 Cloudflare Access 网页登录或 Service Token。Access 默认登录策略可能返回重定向或 HTML，App 无法登录。当前最小方案是 HTTPS Tunnel 加随机 API 令牌，按需在 Cloudflare 侧加限流。组织若强制 Access，应另行实现设备用户认证，不应把组织共享密钥硬编码进 App。

只测试可用 `cloudflared tunnel --protocol http2 --url http://127.0.0.1:8787`。Quick Tunnel 地址临时，不适合长期绑定手机。启用后分别在蜂窝网络与常用 Wi-Fi 验证 401、正确令牌与持续刷新。

若采用 IPv6，使用非临时全局 IPv6 更新域名 AAAA，验证学校上游和主机入站策略，再参考 `deploy/Caddyfile.ipv6.example` 配置有效 TLS 证书。API 继续监听回环。不要关闭手机证书校验。

## 3. GPU 与 CPU 矿工控制

生产环境 GPU Pearl 使用 SRBMiner，OCTOPUS 使用 lolMiner。Krig 1.5.12 切换因服务器 glibc 不兼容而暂停，服务器端不应按下述旧 Krig 方案更新。替换矿工会中断目标 GPU 上的计算；逐卡切换前保留原矿工目录及启动脚本作为回滚路径：

- Pearl：`~/Tongji-Wang/gpu/research-srb-3-8-0/run-pearl-gpu0-tun.sh [kryptex|prl]`
- NiceHash：`~/Tongji-Wang/gpu/research-octopus-1.98a/run-nicehash-gpu0-tun.sh`

SRBMiner 目录名为 `research-srb-3-8-0`，此前实际二进制版本为 3.8.2；目录名不代表版本。当前代码 GPU 路径使用 SRBMiner CPU/GPU 分离参数，OCTOPUS 保持 lolMiner。

```bash
sudo install -m 0644 deploy/gpu-pocket-miner@.service /etc/systemd/system/
sudo install -m 0644 deploy/gpu-pocket-cpu.service /etc/systemd/system/
sudo visudo -cf deploy/gpu-pocket.sudoers
sudo install -m 0440 -o root -g root deploy/gpu-pocket.sudoers /etc/sudoers.d/gpu-pocket
sudo visudo -c
sudo systemctl daemon-reload
```

不要启用矿工开机启动。两个 unit 模板故意没有 Install 段；App 分别管理 GPU0、GPU1 和独立 CPU 服务。

1. 每次只迁移一张卡。用原会话的 Ctrl+C 或原有管理方式停止该卡对应的监督器和子矿工；不要使用 `pkill`、`killall` 或按软件名批量停止。
2. 用 `nvidia-smi --query-compute-apps=gpu_uuid,pid --format=csv` 核实目标 GPU 已释放。API 检测到不属于对应 `gpu-pocket-miner@<id>.service` cgroup 的进程会返回 409，不会自动杀进程；不要停止另一张卡上的工作。
3. 管理员将 `/etc/gpu-pocket/server.toml` 的 `control_enabled` 改为 true，并重启 `gpu-pocket-api.service`。
4. 手机在目标 GPU 卡片上选择算法、矿池及自己核实过的钱包，暂存后确认应用。SRBMiner 全量强度固定为 31；节制强度在控制页配置，默认 10。不设置功耗上限。
5. 检查对应实例，例如 `systemctl status gpu-pocket-miner@0.service` 和 `journalctl -u gpu-pocket-miner@0.service`；核实 GPU 分配、钱包、目标矿池、矿工强度及实际接受份额。首次启动应现场观察 GPU 状态，确认后再迁移另一张卡。

新服务先进入现有 `research-tun`，再以 `joshua` 执行固定矿工参数。GPU0、GPU1 与 CPU 各自使用独立 systemd 服务与状态文件。它不修改代理订阅、DNS、GPU 驱动或固件。新增矿池由管理员编辑 TOML。

### 启用 CPU 挖矿

CPU 使用单独的 `gpu-pocket-cpu.service`，不会与 GPU 服务共用状态或重启。示例配置提供 SRBMiner `randomx` + Monero profile；在真实服务器上先确认 `gpu_root/research-srb-3-8-0/research-srb` 存在，并核实币种、矿池地址、TLS 和钱包格式。其它候选币需管理员为 SRBMiner 支持的精确 CPU 算法各添加 `cpu_profiles` / pools 条目，不能只因 WhatToMine 标记 RandomX 就假定算法变体完全相同。

部署 CPU unit 和最小 sudoers 权限（不会启动服务）：

```bash
sudo install -m 0644 deploy/gpu-pocket-cpu.service /etc/systemd/system/
sudo visudo -cf deploy/gpu-pocket-cpu.sudoers
sudo install -m 0440 -o root -g root deploy/gpu-pocket-cpu.sudoers /etc/sudoers.d/gpu-pocket-cpu
sudo visudo -c
sudo systemctl daemon-reload
```

配置加载后，手机在控制页暂存 CPU profile、矿池、钱包及 1–100% 线程比例（默认 50%），再明确点按启动。SRBMiner 把该百分比定义为 CPU 线程提示，不是严格 CPU 硬限额。首次运行应观察主机 CPU、温度和功耗，并在矿池端确认已接受份额。`systemctl active` 或 API `active` 只证明服务启动，不证明收益或有效算力。API `control_enabled` 为全局写操作开关，打开后 CPU 和 GPU 接口均可写；逐项 sudoers 权限仍需独立安装。

### 启用逐 GPU 控制

GPU0 与 GPU1 使用分开的 sudoers 规则。仅在准备接管相应 GPU 时安装该卡规则；安装权限不会停止旧进程或启动矿工。先在开发机按 `tools/remote-check.ps1` 完成 Rust 检查，并生成 release 二进制；将 `gpu-pocket-server`、`deploy/gpu-pocket-miner@.service`、`deploy/gpu-pocket-gpu0.sudoers`、`deploy/gpu-pocket-api-control.conf` 和 `deploy/enable-control-gpu0.sh` 放入服务器同一个暂存目录。确认 GPU0 旧矿工适合手动切换后，在服务器可信终端以管理员权限执行：

```bash
sudo bash /path/to/staging/enable-control-gpu0.sh /path/to/staging
```

该脚本备份 API 配置和二进制，安装矿工 unit 与仅限 GPU0 的 sudoers 规则，启用 API 控制并重启用户级 API；**不会停止现有进程、启动矿工或启用矿工开机启动**。它为 API 用户服务添加 `NoNewPrivileges=no` 覆盖，因为 `NoNewPrivileges=true` 会阻止 `sudo` 的 setuid 提权。为限制风险，sudoers 只允许 API 用户操作明确列出的 GPU miner systemd 单元；不要扩大该命令白名单。App 手动节制启动允许与其他 GPU 计算任务共存；全量启动仍要求先停止目标 GPU 的外部任务。节制强度不是显存或功耗硬上限，共用时仍可能资源不足或性能下降。不要将管理员密码或令牌粘贴到聊天中。

若要独立启用 GPU1 的 systemd 控制，将 `deploy/gpu-pocket-gpu1.sudoers` 放入服务器暂存目录，并在可信终端执行：

```bash
sudo install -o root -g root -m 0440 /path/to/staging/gpu-pocket-gpu1.sudoers /etc/sudoers.d/gpu-pocket-gpu1
sudo visudo -c
```

这只增加 GPU1 的 start/restart/stop 权限，不会停止 GPU1 上正在运行的外部任务。必须先在原监督器中停止 GPU1 旧任务，App 才能启动托管矿工；GPU1 上仍有外部任务时，API 会拒绝启动。

systemd 仅在进程退出时重启，不检测“进程仍在但没有新任务／份额”。现有 Python 监督器的保活能力没有被默认继承。需要相同能力时，应先扩展矿工 API 适配器，再决定长期迁移。

## 4. 代理节点

专用 Mihomo 控制器为 `127.0.0.1:19097`，未授权请求返回 401。通过 `GPU_POCKET_PROXY_SECRET` 配置专用秘密，不要将 root YAML 或订阅提交 Git。

`RESEARCH_AUTO` 是 Fallback 自动组，App 只能显示自动选择状态。手动切换需要管理员审核现有配置、建立专用 Selector 组、保持矿工流量规则指向该组，并设置服务端的 `proxy.group`。不要直接覆盖运行中的整个 Mihomo 配置；需保留或明确调整现有自动故障切换和禁止 DIRECT 回退策略。

节点选择不保证已有 TCP 连接立即重建。必要时显式重启 GPU0。基础版不自动断开全部代理连接。

## 5. 回退

在 App 或机器终端停止已接管的 `gpu-pocket-miner@<id>.service`，确认该实例 cgroup 已退出，再使用原 launcher 恢复对应 GPU。原脚本没有被改写。将 `control_enabled` 改回 false 并重启 API，可保留监控、关闭所有写操作。

## 临时开发环境

远端 Rust 工具链在 `/tmp/gpu-pocket-tools`，编译副本在 `/tmp/gpu-pocket-build`。为绕过主机 DNS 故障，开发时使用回环地址 `127.0.0.1:17898` 的 SSH 动态 SOCKS 转发，Rust 下载显式使用该代理。这不是生产矿工网络方案。会话结束后关闭转发；没有修改用户默认 Rust 工具链。
