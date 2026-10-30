# 部署与逐 GPU 接管

更新：当前已使用用户级只读 API 与 Quick Tunnel 完成部署，DNS 已恢复。日常使用参见 [当前连接与运维](QUICK_TUNNEL.md)。下文的系统级安装是另一种部署方式，不要在已有用户级 API 占用 8787 时重复启动；真实矿工接管仍未执行。

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

## 2. HTTPS 入口

先修复主机 DNS，再按 [Cloudflare 官方文档](https://developers.cloudflare.com/tunnel/get-started/) 安装 cloudflared、创建命名隧道与域名。参考 `deploy/cloudflared.example.yml`，将域名仅映射到 `http://127.0.0.1:8787`。样例采用 HTTP/2；本次仅验证了 TCP 7844，未验证 QUIC。无需给服务器开放 API 8787 入站。

基础客户端没有集成 Cloudflare Access 网页登录或 Service Token。Access 默认登录策略可能返回重定向或 HTML，App 无法登录。当前最小方案是 HTTPS Tunnel 加随机 API 令牌，按需在 Cloudflare 侧加限流。组织若强制 Access，应另行实现设备用户认证，不应把组织共享密钥硬编码进 App。

只测试可用 `cloudflared tunnel --protocol http2 --url http://127.0.0.1:8787`。Quick Tunnel 地址临时，不适合长期绑定手机。启用后分别在蜂窝网络与常用 Wi-Fi 验证 401、正确令牌与持续刷新。

若采用 IPv6，使用非临时全局 IPv6 更新域名 AAAA，验证学校上游和主机入站策略，再参考 `deploy/Caddyfile.ipv6.example` 配置有效 TLS 证书。API 继续监听回环。不要关闭手机证书校验。

## 3. 逐卡接管

接管会中断目标 GPU 上的计算。先在机器终端核实旧监督器、矿工 PID、运行参数、钱包及 GPU 分配，保留原有启动方式：

- Pearl：`~/Tongji-Wang/gpu/research-srb-3-8-0/run-pearl-gpu0-tun.sh [kryptex|prl]`
- NiceHash：`~/Tongji-Wang/gpu/research-octopus-1.98a/run-nicehash-gpu0-tun.sh`

目录名仍为 `research-srb-3-8-0`，实际二进制已升级至 SRBMiner 3.8.2。不要根据目录名推断版本。

```bash
sudo install -m 0644 deploy/gpu-pocket-miner@.service /etc/systemd/system/
sudo visudo -cf deploy/gpu-pocket.sudoers
sudo install -m 0440 -o root -g root deploy/gpu-pocket.sudoers /etc/sudoers.d/gpu-pocket
sudo visudo -c
sudo systemctl daemon-reload
```

不要启用矿工开机启动。模板故意没有 Install 段；App 只允许操作 GPU0、GPU1 两个实例。

1. 每次只迁移一张卡。用原会话的 Ctrl+C 或原有管理方式停止该卡对应的监督器和子矿工；不要使用 `pkill`、`killall` 或按软件名批量停止。
2. 用 `nvidia-smi --query-compute-apps=gpu_uuid,pid --format=csv` 核实目标 GPU 已释放。API 检测到不属于对应 `gpu-pocket-miner@<id>.service` cgroup 的进程会返回 409，不会自动杀进程；不要停止另一张卡上的工作。
3. 管理员将 `/etc/gpu-pocket/server.toml` 的 `control_enabled` 改为 true，并重启 `gpu-pocket-api.service`。
4. 手机在目标 GPU 卡片上选择算法、矿池、可选功耗上限及自己核实过的钱包，暂存后确认应用。
5. 检查对应实例，例如 `systemctl status gpu-pocket-miner@0.service` 和 `journalctl -u gpu-pocket-miner@0.service`；核实 GPU 分配、钱包、目标矿池及实际接受份额。逐卡功耗参数未经真实矿工环境验证，首次启动应现场观察功耗，确认后再迁移另一张卡。

新服务先进入现有 `research-tun`，再以 `joshua` 执行固定矿工参数。GPU0、GPU1 各自使用独立 systemd 实例与状态文件。它不修改代理订阅、DNS、GPU 驱动或固件。新增矿池由管理员编辑 TOML。

systemd 仅在进程退出时重启，不检测“进程仍在但没有新任务／份额”。现有 Python 监督器的保活能力没有被默认继承。需要相同能力时，应先扩展矿工 API 适配器，再决定长期迁移。

## 4. 代理节点

专用 Mihomo 控制器为 `127.0.0.1:19097`，未授权请求返回 401。通过 `GPU_POCKET_PROXY_SECRET` 配置专用秘密，不要将 root YAML 或订阅提交 Git。

`RESEARCH_AUTO` 是 Fallback 自动组，App 只能显示自动选择状态。手动切换需要管理员审核现有配置、建立专用 Selector 组、保持矿工流量规则指向该组，并设置服务端的 `proxy.group`。不要直接覆盖运行中的整个 Mihomo 配置；需保留或明确调整现有自动故障切换和禁止 DIRECT 回退策略。

节点选择不保证已有 TCP 连接立即重建。必要时显式重启 GPU0。基础版不自动断开全部代理连接。

## 5. 回退

在 App 或机器终端停止已接管的 `gpu-pocket-miner@<id>.service`，确认该实例 cgroup 已退出，再使用原 launcher 恢复对应 GPU。原脚本没有被改写。将 `control_enabled` 改回 false 并重启 API，可保留监控、关闭所有写操作。

## 临时开发环境

远端 Rust 工具链在 `/tmp/gpu-pocket-tools`，编译副本在 `/tmp/gpu-pocket-build`。为绕过主机 DNS 故障，开发时使用回环地址 `127.0.0.1:17898` 的 SSH 动态 SOCKS 转发，Rust 下载显式使用该代理。这不是生产矿工网络方案。会话结束后关闭转发；没有修改用户默认 Rust 工具链。
