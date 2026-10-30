# 网络排查与选型

检查日期：2026-10-30，Asia/Shanghai。本文保留首次只读排查的结论；项目根目录 `network-probe.json` 已更新为 DNS 修复后的复测结果。首次排查没有改动系统，后续修复和实际隧道部署记录见文末。

## 检查结果

SSH 别名 `2-titan` 指向 `100.94.37.27:99`，用户为 `joshua`。该地址属于当前 Tailscale 网络，连接成功。远端主机名为 `252-2titanx`，实际 GPU 为两张 NVIDIA GeForce RTX 3080 Ti，各 12288 MiB 显存。`2-titan` 是机器别名，不代表 GPU 型号。

服务器无线接口拥有 `2001:da8:8002:6bd1::/64` 前缀的公网 IPv6，包括多个临时地址、已弃用地址及一个非临时动态地址。服务器也有 IPv6 默认路由。公网 AAAA 若采用此方案，应使用非临时地址，并处理前缀变化，不能复制一个即将失效的临时地址。

开发机只有链路本地 IPv6 和 Tailscale ULA，没有公网 IPv6。尝试连接服务器非临时 IPv6 的 SSH 99 端口得到 Windows 10051，表示开发机没有可用网络路径。此结果不能证明服务器入站端口被封锁。服务器直接连接 `2606:4700:4700::1111:443` 超时；单个目标失败也不能证明所有 IPv6 目标都不可用。

服务器解析 `region1.v2.argotunnel.com`、`region2.v2.argotunnel.com`、`api.trycloudflare.com` 和 `github.com` 时出现 `Temporary failure in name resolution`。开发机先解析隧道域名，再让服务器直连以下端点，均成功完成 TCP 握手：

- `198.41.192.107:7844`
- `198.41.192.167:7844`
- `198.41.200.113:7844`
- `198.41.200.13:7844`

这些 IP 是检查时的 DNS 结果，不应永久写死。未测 UDP 7844；未启动 cloudflared，也未完成 TLS 握手、隧道注册或手机移动网络访问测试。主机未安装 cloudflared。

## 选择

推荐 Cloudflare Tunnel 作为公网入口。它由服务器发起出站连接，无需学校／上游路由器开放入站端口；手机可以通过普通 HTTPS 访问，不依赖手机当前网络是否支持 IPv6。本次 TCP 7844 检查有利于采用 HTTP/2 隧道，但 DNS 和完整建隧道仍是部署前置条件。

Cloudflare 官方文档要求允许出站 7844：QUIC 使用 UDP，HTTP/2 使用 TCP。命名隧道发布应用需要 Cloudflare 账户及域名。Quick Tunnel 不需要账户或域名，但地址临时，只用于开发测试。见 [Cloudflare Tunnel](https://developers.cloudflare.com/tunnel/)、[安装与前提](https://developers.cloudflare.com/tunnel/get-started/)、[Quick Tunnels](https://developers.cloudflare.com/tunnel/get-started/quick-tunnels/)。

IPv6 直连可以减少中转依赖，但目前缺少可用性证据。需要同时验证服务器出站、学校入站策略、手机蜂窝／Wi-Fi 的公网 IPv6、稳定 AAAA 和 TLS 证书。仅有 `ip -6 addr` 不能证明整条连接可用。

## 下一步验证

1. 先由管理员检查主机的 `resolvectl status`、`/etc/resolv.conf`、本机 DNS 监听及上游 DNS 策略。不要覆盖 `research-tun` 的 DNS；矿工命名空间与主机是不同网络环境。
2. 修复后运行 `python tools/network_probe.py --ssh 2-titan`，确认隧道域名能在主机解析。
3. 使用命名隧道，将 `gpu.example.com` 仅映射到 `http://127.0.0.1:8787`。先启动只读 API，再验证未带令牌返回 401、有效令牌返回真实 GPU 数据。
4. 在手机蜂窝和常用 Wi-Fi 上各进行实际 HTTPS 请求与持续刷新测试。中国大陆网络的实际延迟和稳定性必须以此为准。
5. 若 TCP 可用但 QUIC 不稳定，配置 `protocol: http2`。当前样例已采用该值。TCP 成功不保证应用层一定成功。

如果选择 IPv6，应从独立 IPv6 网络测试 HTTPS 443，再考虑切换主入口。通过域名和有效 TLS 证书访问比直接使用 IP 更易维护。

## 复用评估

- [tokio-rs/axum](https://github.com/tokio-rs/axum)：用于 Rust HTTP API 和中间件，避免自建网络框架。
- [NVIDIA/dcgm-exporter](https://github.com/NVIDIA/dcgm-exporter)：适合 Prometheus GPU 指标采集，但不包含本机钱包、算法和现有命名空间控制。当前两卡基础版直接使用已安装且实测正常的 `nvidia-smi`，以后可增加 Prometheus。
- [cloudflare/cloudflared](https://github.com/cloudflare/cloudflared)：复用官方隧道客户端，不自行实现穿透。
- [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo)：复用现有控制 API。专用控制器 `127.0.0.1:19097` 在无凭据请求时返回 401，证明需要单独授权；没有读取或复制 root 配置内的秘密。`RESEARCH_AUTO` 是当前部署文档记载的 Fallback 组，不能假设 PUT 会让它变成手动选择组。

没有发现可直接满足“Flutter + Rust + 现有 GPU0 命名空间和启动脚本”的已验证成品，因此采用这些组件并编写薄适配层。

## 2026-10-30 17:28 更新：DNS 修复与隧道验证

主机 `/etc/resolv.conf` 正常指向 systemd-resolved 的 `127.0.0.53`，但 resolved 的无线接口上游被运行时设置为 `198.18.0.2`，且该地址的路由走学校网关，没有本机代理 TUN 接管。对系统 stub 和该虚拟地址的直接 DNS 查询均超时；学校的 `202.120.190.208`、`202.120.190.108` 及四个公共解析器均能返回正确结果。NetworkManager 的连接配置仍为自动 DNS，DHCP 数据中也保留着正确的学校地址。因此故障明确发生在运行时 DNS 覆盖层；未确定具体哪个进程、何时写入了覆盖值，不应直接归因于某个软件版本。

用户授权后通过 sudo 执行项目中的 `deploy/repair-host-dns.sh`，只恢复无线接口的 DHCP DNS 并清理缓存，没有编辑 resolv.conf、重启网络或改变 `research-tun`。主机 DNS stub 随后返回成功，实测约 2 ms。备份和可执行回滚脚本位于 `/var/backups/gpu-pocket-dns/20261030-172126-983516/`。

官方 cloudflared 2026.10.6 从 `pkg.cloudflare.com` 下载，在用户目录解包，未添加系统软件源或安装全局软件包。DEB SHA256 为 `13d9a724a4d4271958a3728982ee983870a100959a159d2fa263b3450d0be72a`；二进制 SHA256 为 `1d356dac769d0dbf62ebe588c4034824bbff98d5790c2f844cc99e0e2f57462d`，均与官方发行信息一致。

Quick Tunnel 成功通过 TCP HTTP/2 注册到 `hkg01`。预检查显示 region1 QUIC 成功、region2 QUIC 失败，两个区域 HTTP/2 均成功，因此继续固定使用 HTTP/2。开发机和服务器分别通过公网 HTTPS 验证：匿名 401、正确令牌 200、矿工控制 403、真实 GPU 数量为 2、采样错误为空。当前地址和管理命令见 QUICK_TUNNEL.md。
