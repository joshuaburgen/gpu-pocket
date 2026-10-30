# 当前 Quick Tunnel 连接与运维

部署时间：2026-10-30，Asia/Shanghai。当前面向 Android 使用，服务器提供真实只读监控。

## 手机填写

当前服务地址：

```text
https://federation-privileges-honey-submitting.trycloudflare.com
```

API 令牌位于开发机项目的 `.local/connection.json` 的 `token` 字段。该文件已被 Git 忽略，不应提交或公开。远端原始凭据在 `/media/new_data3/.config/gpu-pocket/api.env`，权限为 0600。App 连接页分别填写 URL 和令牌；不要将整个 JSON 粘贴进令牌输入框。

当前 API 的 `control_enabled` 为 false，公网可监控两张 GPU。矿工启停、算法／钱包切换和代理节点写入均被拒绝，原有工作未接管。Android APK 尚待安装 Android SDK / JDK 后构建。

地址在 Quick Tunnel 进程重启后可能变化。不要因 App 连接失败而直接重启隧道；先检查当前地址。API 服务重启通常不改变隧道地址。

## 服务状态与日志

使用 `ssh 2-titan` 登录，以 joshua 执行：

```bash
systemctl --user status gpu-pocket-api.service gpu-pocket-quick-tunnel.service
journalctl --user -u gpu-pocket-api.service -n 30 --no-pager
journalctl --user -u gpu-pocket-quick-tunnel.service -n 40 --no-pager
python3 ~/.local/share/gpu-pocket/quick_status.py --export
```

最后一条命令仅输出地址和不含令牌的就绪状态，将完整连接信息保存到远端私有 `~/.config/gpu-pocket/connection.json`。如果隧道已重启，应重新导出，并把新的 URL 更新到手机；API 令牌不因隧道重启而自动改变。

两个用户服务均已 enable，且 joshua 已启用 linger，使其不依赖 SSH 会话保持运行。服务重启或整机重启后仍可能获得新的临时 URL；本次没有重启整机验证开机路径。

## 文件位置

- 程序：`~/.local/share/gpu-pocket/{gpu-pocket-server,cloudflared}`
- 配置和凭据：`~/.config/gpu-pocket/`
- 状态目录：`~/.local/share/gpu-pocket/state/`
- 用户 unit：`~/.config/systemd/user/gpu-pocket-{api,quick-tunnel}.service`
- 监听：API `127.0.0.1:8787`，隧道指标 `127.0.0.1:18787`。均未开放公网监听端口。

## DNS 回归检查

```bash
python3 ~/.local/share/gpu-pocket/dns_probe.py --require-system
```

系统 stub `127.0.0.53` 应成功。旧虚拟地址 `198.18.0.2` 仍超时是预期结果，它已不再是无线接口 DNS。探测脚本会在系统 stub 失败时以非零状态退出。

若运行时 DNS 再次被外部程序改写，先检查 `resolvectl dns wlx6c1ff738d2cf`。现有修复脚本会重新验证 DHCP DNS、备份当时状态并恢复接口配置：

```bash
sudo bash ~/.local/share/gpu-pocket/repair-host-dns.sh
```

本次修复的回滚命令为：

```bash
sudo bash /var/backups/gpu-pocket-dns/20261030-172126-983516/rollback.sh
```

回滚会恢复当时失效的 DNS，因此仅在明确需要恢复原状态时使用。

## 停止与禁用

```bash
systemctl --user disable --now gpu-pocket-quick-tunnel.service gpu-pocket-api.service
```

该命令只停止本项目的公网入口和只读监控。不会停止原有矿工、research-tun 或 GPU1 工作。如果要撤销 linger，先确认该用户没有其他需要后台运行的服务，再执行 `sudo loginctl disable-linger joshua`。

## 公网检查

在开发机使用私有连接文件运行：

```powershell
python tools/check_connection.py .local/connection.json --output docs/quick-tunnel-check.json
```

本机 Anaconda 的 Windows 系统证书导入存在 `ASN1: NOT_ENOUGH_DATA` 问题。验证时通过 `--ca-file E:/Dev/Anaconda/Lib/site-packages/certifi/cacert.pem` 显式使用现有可信 CA 包，未关闭 TLS 验证。服务器使用其系统 CA 同样通过。此问题不属于服务器证书或 Android 客户端。
