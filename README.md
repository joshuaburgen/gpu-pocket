# GPU Pocket

用于在手机监控 `2-titan`，并分别受控管理 `Tongji-Wang/gpu` 下 GPU0、GPU1 工作流的基础项目。手机端为 Flutter，服务器端为 Rust / Axum。

## 当前功能

- 每 5 秒读取 GPU 利用率、显存、温度、功耗和风扇转速；不支持的数值显示为空，不伪造为 0。
- 切换 Pearl / pearlhash（SRBMiner）与 NiceHash / OCTOPUS（lolMiner），选择服务端允许的矿池，填写钱包或矿池账户。
- 每张 GPU 独立暂存算法、矿池、钱包和可选功耗上限；确认应用后仅操作对应的 `gpu-pocket-miner@<id>.service`。
- 每张 GPU 可单独启用自动策略：GPU 利用率低于 5% 连续 30 分钟后启动 Pearl 节制模式；北京时间 01:00–08:00 可运行深夜模式；节制／日间提醒检测到外部计算进程时暂停托管矿工并按设置发送提醒。深夜模式按要求忽略计算占用和外部进程，只受显存保护。全局生产控制及每卡策略默认关闭。
- 节制模式可设置 SRBMiner 强度和可选功耗上限；深夜模式忽略功耗／强度，只使用显存保护。可用显存低于阈值时服务端停止自动矿工，默认阈值 2048 MiB，每 5 秒检查一次。这是停矿保护而非显存硬上限，无法保证轮询间隙不会短时超限。
- 手机推送需配置 Firebase 客户端参数和服务端服务账号；没有 Firebase 凭据时自动控制仍可运行，但通知无法投递。真实矿工环境中的逐卡功耗参数仍须按部署文档现场验证。
- 读取指定 Mihomo 代理组。只有 Selector 组允许手选节点；Fallback 组显示自动选择状态。
- API 使用 Bearer 令牌；手机将连接信息保存到系统安全存储。公网连接强制 HTTPS。
- 演示模式使用显式标记的模拟数据，不启动矿工、不调用真实代理控制器。

## 网络结论

IPv6 最新进展：校内电脑及校园 Wi-Fi 手机已能直连；手机蜂窝网络及连接手机热点的电脑均无法主动连接服务器 IPv6。具体过滤节点尚未确定。见 [IPv6 检查记录](docs/IPV6.md)。不要把临时 HTTP 测试地址填入 App。

**2026-10-30 更新：主机 DNS 已修复，Cloudflare Quick Tunnel 与只读 API 已部署并通过公网验证。** 使用方法见 [当前连接与运维](docs/QUICK_TUNNEL.md)，验证结果见 [公网检查记录](docs/quick-tunnel-check.json)。无需域名或 Cloudflare 账户。Android 是当前首要客户端目标。

优先使用 **Cloudflare Tunnel + HTTPS**。首次排查发现主机 DNS 无法解析隧道域名。后续已将无线网卡运行时 DNS 从失效的 `198.18.0.2` 恢复为学校 DHCP 下发的两个解析器。实际 Quick Tunnel 已通过 HTTP/2 注册香港节点；从开发机和服务器分别访问公网 HTTPS 均通过鉴权及真实 GPU 采样检查。

IPv6 作为备选。服务器、手机及热点电脑均已确认有公网 IPv6，但蜂窝到服务器的主动连接失败。服务器主动 ping 手机可以收到回复，尚不能据此确定校园防火墙或某个路由节点就是原因。当前跨网络连接使用 Quick Tunnel。

SSH 的 `100.94.37.27:99` 是现有 Tailscale 地址，已验证可用。它可以作为部署、调试的现成通道。完整证据和验证顺序见 [网络排查](docs/NETWORK.md)。

## 目录

```text
mobile/       Flutter 手机客户端，Android / iOS 工程
server/       Rust API、GPU 采样、固定矿工启动器、测试
deploy/       systemd、最小 sudoers、Cloudflare 和 IPv6 HTTPS 示例
tools/        只读网络探测和远程编译脚本
docs/         网络证据、部署、接口和交接状态
```

## 本地演示

需要 Rust stable、Flutter stable。以下演示使用独立的 `/tmp` 状态目录，不依赖 GPU 或 systemd。示例为 Linux / macOS shell；Windows 可在 WSL 中运行 Rust 部分。

```bash
cp server/config.example.toml /tmp/gpu-pocket-demo.toml
# 编辑 /tmp/gpu-pocket-demo.toml：
# state_dir = "/tmp/gpu-pocket-demo-state"
# control_enabled = true
# gpu_root 保持任意绝对路径，演示模式不会执行该目录下的文件。
export GPU_POCKET_MODE=demo
export GPU_POCKET_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
cargo run --manifest-path server/Cargo.toml -- /tmp/gpu-pocket-demo.toml
```

另开终端：

```bash
cd mobile
flutter pub get
flutter run
```

连接页填写 `http://127.0.0.1:8787` 和生成的令牌。Android 真机通过 USB 可运行 `adb reverse tcp:8787 tcp:8787`，将手机回环地址转发至电脑。Android 模拟器也可使用 `adb reverse`。手机上的 `127.0.0.1` 默认指手机，不是服务器。

当前开发机下载的 Flutter SDK 放在 `.tools/flutter`，已被 Git 忽略。可用 `.tools/flutter/bin/flutter.bat` 运行。中国镜像通过当前终端的 `FLUTTER_STORAGE_BASE_URL=https://storage.flutter-io.cn`、`PUB_HOSTED_URL=https://pub.flutter-io.cn` 设置，不修改全局环境。

## 验证与打包

```bash
cargo test --manifest-path server/Cargo.toml
cargo build --release --manifest-path server/Cargo.toml
cd mobile
dart format --output=none --set-exit-if-changed lib test
flutter analyze
flutter test
flutter build apk --debug
```

Android 打包需要 JDK 与 Android SDK。本地构建脚本和手动安装步骤见 [Android 构建与真机验证](docs/ANDROID.md)。iOS 构建、真机签名需要 macOS / Xcode；本项目没有正式发布签名证书。发布前自行设置包名、签名和发布渠道。

真实服务器安装与接管步骤见 [部署文档](docs/DEPLOYMENT.md)。当前已安装用户级只读 API 和 Quick Tunnel 服务，没有停止旧监督器或启用矿工控制。真实接管需另行按部署文档执行。

## 边界

这是基础控制台，不是完整矿场系统。当前不提供收益统计、份额／算力曲线、温控降频或任意命令执行。systemd 只对进程退出提供重启，不继承现有 Python 监督器的“长时间无任务／无份额”检测。新增算法需要扩展服务端固定适配器，不能从手机上传 shell 命令。钱包字段只接受接收地址或矿池账户，不存私钥或助记词。
