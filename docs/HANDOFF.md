# GPU Pocket 项目交接报告

更新日期：2026-10-31

## 1. 项目状态摘要

GPU Pocket 由 Linux Rust API 和 Flutter Android 客户端组成。当前已完成 GPU0/GPU1 状态与控制、GPU 进程展示、目录外新进程告警、手机端竖屏和 FCM 状态分层。Rust 与 Flutter 的针对性检查已通过。

FCM 目前只验证到“服务端请求被 Firebase FCM HTTP v1 接受”，**尚未验证 Android 手机实际显示通知**。不能把服务器凭据配置、Token 登记或 FCM 接受消息当作端到端通知成功。

本报告记录工作区中尚未提交的开发与部署工作。不要在交接时清理、重置或覆盖这些改动。

### Krig 升级状态（2026-10-31）

- 官方项目为 [Kryptex Krig](https://github.com/kryptex/krig-miner)；最新稳定版为 1.5.12。官方 Linux x64 release SHA-256 为 `6c456f6540a26ab2dd5fa02434d5827f09d19e06001ef0e6eef5ac1f80b17cea`。
- 服务器是 Ubuntu 20.04、glibc 2.31；官方 Krig 二进制要求 glibc 2.32、2.33、2.34、2.35。启动 `--version` 即被动态链接器拒绝，尚未运行 Krig，也未升级系统库。
- 用户选择暂停切换、维持生产矿工。GPU0/GPU1 的 GPU Pocket systemd 矿工服务仍为 active；生产配置、已安装 API binary、逐卡 state 和矿工进程均未被本轮改动。
- 本地工作区已准备 Krig 适配代码、Kryptex SSL profile 和手机端控制 UI；Rust 检查通过（16 项测试、Clippy、debug/release build），Flutter analyze/test 通过，Firebase-enabled debug APK 已构建。ADB 当时没有连接设备，因此 APK 未安装到手机。
- **不要部署当前工作区的 Krig 配置或 server binary**：生产主机的 glibc 兼容方案尚未确定。官方包已从 `/tmp` 清理。后续只有在用户明确选择兼容路线并完成验证后，才可进行生产逐卡切换。

## 2. 架构与主要行为

- **手机端**：Flutter Android App，包名 `com.tongjiwang.gpu_pocket`。通过 HTTPS 调用 Rust API；服务器 URL 和 API 令牌存储在 Android 安全存储中。
- **服务器端**：Rust API 读取 GPU 状态并执行受服务端开关约束的控制操作。生产服务由用户级 systemd 管理。
- **GPU 进程**：服务器调用 `nvidia-smi` 读取 GPU 进程，过滤 Xorg，返回 PID、进程类型、显存、路径和是否位于 GPU 工作目录内。首次采样建立告警基线，之后只对新出现的目录外进程告警，避免持续进程重复告警。
- **手机刷新**：状态约每 15 秒刷新；进入后台停止轮询，回到前台恢复。控制卡状态不再因状态轮询的短暂 loading 而变灰。
- **FCM 状态**：客户端分别展示服务器凭据配置、Firebase 初始化/Token 获取、Token 向 Rust API 登记，不允许轮询到的 `push_configured=true` 覆盖客户端错误。
- **安全边界**：服务账号私钥只放服务器；API 令牌不编译进 APK；生产控制开关仍受服务器配置约束。

主要代码与说明：

- [main.dart](../mobile/lib/main.dart)：手机 UI、GPU 进程、客户端推送状态和注册逻辑。
- [gpu.rs](../server/src/gpu.rs)、[main.rs](../server/src/main.rs)：GPU 进程采样、状态和目录外进程告警。
- [push.rs](../server/src/push.rs)：Firebase OAuth 与 FCM HTTP v1 发送。
- [tests.rs](../server/src/tests.rs)：Rust 回归测试。
- [build_android.ps1](../tools/build_android.ps1)：Android arm64 APK 构建脚本。
- [remote-check.ps1](../tools/remote-check.ps1)：远端 Rust 格式、测试、Clippy 和构建检查。
- [ANDROID.md](ANDROID.md)、[DEPLOYMENT.md](DEPLOYMENT.md)、[API.md](API.md)：客户端构建、服务器部署及 API 说明。

## 3. 开发机环境与 Android 构建

### 环境

- 开发机：Windows 11；仓库：`D:\work\github\gpu-pocket`。
- Flutter、Android SDK、Gradle 缓存放在仓库忽略目录 `.tools`，不应提交。
- 使用 JDK 17 或更高版本。当前工程采用 Android SDK 36、Build Tools 36.0.0、NDK 28.2.13676358 和 Gradle 9.3.1。
- 本次构建验证版本：Flutter 3.47.9 / Dart 3.13.7。

### 构建与验证

在仓库根目录执行：

```powershell
./tools/build_android.ps1 -JavaHome '<JDK 目录>' -DefaultUrl '<HTTPS API 源站>'
```

配置 Firebase 客户端时，通过构建脚本传入完整的四项客户端参数：`-FcmApiKey`、`-FcmAppId`、`-FcmSenderId`、`-FcmProjectId`；必须全部提供或全部留空。实际值不要写入文档、命令记录或提交内容。APK 默认输出到 `mobile/build/app/outputs/flutter-apk/app-debug.apk`。

构建后可执行：

```powershell
cd mobile
../.tools/flutter/bin/flutter.bat analyze
../.tools/flutter/bin/flutter.bat test
```

Android 真机安装/日志采集使用仓库内的 ADB：

```powershell
./.tools/android-sdk/platform-tools/adb.exe devices -l
./.tools/android-sdk/platform-tools/adb.exe install -r mobile/build/app/outputs/flutter-apk/app-debug.apk
```

当前验证结果：`flutter analyze` 和 3 项 Flutter 测试通过；Firebase-enabled debug APK 已构建并安装。Android Activity 已锁定竖屏。

### Android/Firebase 集成说明

- Firebase 客户端配置文件位于仓库根目录 `google-services.json`，在本机 Git exclude 中，不应提交。
- 包名与 Firebase Android 客户端匹配。当前没有 `firebase_options.dart`，也没有应用 Google Services Gradle 插件；`build_android.ps1` 从 Firebase 客户端配置读取值并作为 Dart defines 传入，App 通过 Dart `FirebaseOptions` 手动初始化。
- 因未使用 Gradle 自动配置，启动时 `FirebaseInitProvider` 提示缺少默认选项是预期行为；不能据此判断 Dart 侧手动初始化失败。
- 当前依赖为 `firebase_core ^5.1.0`、`firebase_messaging ^17.0.1`。
- 不要把 Firebase 服务账号 JSON 传给 Android 构建脚本。服务账号密钥只供服务器使用。

### 构建踩坑

- Windows 上 Kotlin 增量缓存曾因工程路径变化出错；`mobile/android/gradle.properties` 中保留 `kotlin.incremental=false` 作为已验证的 workaround。
- `tools/build_android.ps1` 会在本次 PowerShell 进程内设置 Java、SDK、Gradle 和 Flutter 镜像变量；单独运行 Flutter 命令时需自行设置等价环境，或按 [ANDROID.md](ANDROID.md) 操作。
- Firebase 参数需四项齐全；不要只传部分参数。
- debug APK 使用调试签名；当前 release 构建也不能作为正式签名发布包。正式分发前需配置发布签名。

## 4. Rust 检查、构建与服务器部署

### 远端检查

远端 SSH alias 为 `2-titan`。在 Windows PowerShell 仓库根目录运行：

```powershell
./tools/remote-check.ps1
```

脚本把 Rust manifest、lockfile、示例配置和源码复制到服务器 `/tmp/gpu-pocket-build`，再依次运行：

```text
cargo fmt -- --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo build --release --locked
```

本次 Rust 检查通过：格式检查、15 项测试、Clippy、debug 和 release 构建均成功。该脚本只在临时目录验证，不会部署服务或操作矿工。

### 生产服务与配置位置

- 用户级 systemd 服务：`gpu-pocket-api.service`。
- 主配置：`/media/new_data3/.config/gpu-pocket/server.toml`。
- 环境文件：`/media/new_data3/.config/gpu-pocket/api.env`。
- 运行状态：`/media/new_data3/.local/share/gpu-pocket/state`。
- 服务账号路径通过 `GPU_POCKET_FCM_CREDENTIALS` 提供；实际服务账号文件路径及内容不要写入仓库。
- 服务器 SSH alias：`2-titan`。部署前后应检查服务状态、API 鉴权和 GPU 状态；不要在构建验证时误启生产控制。

### FCM 专用代理修复

- 服务器直连 Google OAuth/FCM 曾超时；已有的一个本地 Mihomo 监听出口 TLS 握手失败。
- 服务器上的另一个 Mihomo 出口成功建立 Google HTTPS 连接。当前 Rust 服务支持用 `GPU_POCKET_FCM_PROXY` 单独为 FCM HTTP 客户端指定代理；普通 API HTTP 客户端不受影响。
- 生产用户服务已安装 systemd drop-in `70-fcm-proxy.conf`，设置该环境变量指向服务器可用的 Mihomo 出口。实际代理地址属于机器部署配置，文档不记录具体内部地址。
- 部署后服务保持 active，未认证 API 仍返回 401，服务器登记 Token 数量为 1。
- 部署前生产 binary 备份：`/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-fcm-proxy-20261031T114353Z`。回滚前先确认当前服务版本和 drop-in 内容；需要回滚代理功能时，同时评估新旧 binary 对配置/状态的兼容性，不要直接删除状态目录。

## 5. FCM 诊断结论与当前验收状态

### 客户端 Token 获取

曾观察到 `Firebase Installations Service is unavailable` / `FCM Registration failed`。实测发现 GPU Pocket 在 v2rayNG 的分应用代理中被排除；当时该 UID 对 Firebase Installations 域名的 DNS 解析失败。经授权把 GPU Pocket 纳入分应用代理并重连 VPN 后，手机成功获取 FCM Token，随后 Rust API 登记数量变为 1。

Google Play 服务包已安装且启用；设备是 Redmi K70 至尊版，Android 包名与 APK 一致。手机上实际运行的是 v2rayNG TUN，而非 Clash。Google Play 服务的当前分应用代理选择尚未在最近一轮确认，不要据此假设其一定包含或排除在代理内。

### 服务器发送与手机显示

- 服务端已存在 Firebase 服务账号配置；`push_configured=true` 仅说明服务器凭据配置存在。
- 服务端对已登记 Token 发出单条诊断消息后，Firebase FCM HTTP v1 接受了请求。
- 用户没有在手机看到通知；ADB 检查也没有发现活动通知记录。
- 因此**后台/锁屏端到端通知验收未通过**。FCM 接受请求不等于 Android 已接收、展示或用户已看到通知。
- Android 通知权限此前显示 granted，FirebaseMessagingService receiver 存在，但这些也不足以证明通知投递成功。
- 后续若继续排查，需先重新确认 v2rayNG 对 `com.google.android.gms` 的分应用代理状态，再由用户确认后重连 VPN。之后使用带唯一标识的诊断通知，在手机后台或锁屏时发送，并同时核对 Android 通知记录及用户实际屏幕反馈。不要擅自切换全局代理、DNS 或其他 VPN 设置。

### 状态判断表

| 阶段 | 当前结论 |
|---|---|
| Firebase 客户端参数与 Android 包名 | 已核对匹配 |
| 手机 Firebase 初始化与 FCM Token 获取 | 曾成功获取 |
| Token 向 Rust API 登记 | 曾成功；服务器记录数量为 1 |
| 服务器 FCM 凭据配置 | 已配置 |
| 服务端 FCM 出站连接 | 专用代理已部署；FCM HTTP v1 接受诊断请求 |
| Android 后台/锁屏实际展示 | 未验证成功；用户未看到通知 |

## 6. 其他已完成事项

- GPU 卡片展示进程 PID、显存和路径，并排除 Xorg。
- 服务器以首次采样建立基线，避免启动时对既有目录外进程误报；后续新目录外 GPU 进程只告警一次。
- 控制卡以绿色表示 GPU Pocket 矿工运行、黄色表示其他进程占用、灰色表示空闲；外部进程状态无法确认时显示橙色。
- App 轮询期间控制项不再短暂变灰；操作中保护仍保留。
- 手机端 Activity 仅允许竖屏。

## 7. 工作区交接注意事项

交接时工作区存在未提交改动，至少包括：

- 修改：`deploy/enable-control-gpu0.sh`、`docs/API.md`、`docs/DEPLOYMENT.md`、`mobile/README.md`、`mobile/android/app/src/main/AndroidManifest.xml`、`mobile/android/gradle.properties`、`mobile/lib/main.dart`、`server/src/gpu.rs`、`server/src/main.rs`、`server/src/push.rs`、`server/src/tests.rs`。
- 未跟踪：`deploy/gpu-pocket-api-control.conf`、`deploy/gpu-pocket-gpu1.sudoers`。
- 本交接报告为新增文件 `docs/HANDOFF.md`。

以上清单来自交接时的 Git 状态；继续工作前应重新检查 `git status`，并保留其他协作者的变更。不要因为这些改动未提交而执行 reset、checkout/restore 或清理。

## 8. 凭据与隐私要求

- 不要把完整 FCM Token、Firebase API Key、服务账号 JSON/私钥、API 令牌、代理订阅地址或完整代理节点配置写入文档、日志或提交。
- 排查 Token 时只记录是否存在和数量；记录错误时保留必要异常类别，但先检查异常消息中是否包含凭据。
- 服务器端 FCM 失效 Token 错误信息不应包含 Token 原值。
- 生产配置、systemd drop-in、服务账号和运行状态属于部署侧数据；修改前备份，避免把服务器机密拷入仓库。
