# GPU Pocket 项目交接报告

更新日期：2026-11-03

## 1. 项目状态摘要

GPU Pocket 由 Linux Rust API 和 Flutter Android 客户端组成。当前支持 GPU0/GPU1 状态与控制、GPU 进程展示、目录外新进程告警、手机端竖屏、FCM 状态分层，以及 SRBMiner 启动前可选算法强度（1–31）。小规模强度用可选开关和 Slider 设置；留空时不传参数，由 SRBMiner 使用默认值。

FCM 目前只验证到“服务端请求被 Firebase FCM HTTP v1 接受”，**尚未验证 Android 手机实际显示通知**。不能把服务器凭据配置、Token 登记或 FCM 接受消息当作端到端通知成功。

本报告记录工作区中尚未提交的开发与部署工作。不要在交接时清理、重置或覆盖这些改动。

### 最新工作区状态（2026-11-02）

- 监控卡片中重复显示的 `GPU0 · active` / `GPU1 · active` 小字已移除；卡片内已有状态面板保留。
- 强度控制曾因 `/api/v1/catalog` 未返回 GPU profile 的 `miner` 类型，导致客户端把 SRBMiner profile 判断为不支持；API 现返回公开 `miner` 类型，客户端以该字段显示 Slider。不要把二进制路径或代理凭据放入 catalog。
- zww 的 `research-tun-network`、`research-tun-proxy`、`research-tun` 均已启动；代理 YAML 中修复了拼接的节点/组字段及重复代理名。最终验证：配置检查通过、三个 unit 均 active、`research-tun` 默认路由经 `rt-peer`，命名空间 HTTPS 测试通过。未启动 GPU miner，也未停止现有 API/CPU 服务。
- 最新 APK 构建位置：`mobile/build/app/outputs/flutter-apk/app-release.apk`，2026-11-02 19:04 构建，19:13 安装到 Redmi 2407FRK8EC，ADB 确认安装成功。
- 本次验证：Flutter API tests 通过；Rust 19 项测试、`cargo fmt --check` 通过。Flutter analyze 仍有 13 条既有 warning/info；完整 widget 测试此前有首屏连接文案断言失败。
- 2026-11-02 zww GPU0 短测：通过 API 启动 Pearl 矿工 25 秒后停止，服务最终 inactive。进程确实处于 `research-tun` netns；测试期间未观察到该进程的已建立 TCP socket，故只验证 namespace 绑定，不代表矿池连接或 share 验收成功。namespace 独立 HTTPS 检查成功。
- 更新后的 API binary 已部署到 zww system service 和 2-titan user service；两台主机的 `/api/v1/catalog` 均已核实返回 `pearl:srb-pearl`、`octopus:lol-octopus`，API 服务 active。旧 binary 回滚副本分别保存在 `/opt/gpu-pocket/gpu-pocket-server.pre-slider-20261102` 和 `/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-slider-20261102`。部署只重启 API，没有改动 GPU unit；zww GPU0 当前 inactive，2-titan GPU0 当前 active。
- 2026-11-03 空闲自启阈值改为 10 分钟；2-titan 远端格式检查、24 项 Rust 测试、Clippy、debug/release 构建全部通过。相同 release binary 已部署至 `2-titan` 用户级 API 和 zww system API；服务均 active，未认证 API 均返回 401，GPU miner units 均保持 inactive。回滚副本分别为 `/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-idle10-20261103T004559` 和 `/opt/gpu-pocket/gpu-pocket-server.pre-idle10-20261103T004646`。本次没有等待真实空闲触发，以免测试期间启动全量强度 31 的矿工。zww 两卡自动策略已启用；策略文件中的旧 intensity 1/3 不参与自启，仍按 31 启动。
- 2026-11-03 空闲计时与 CPU 展示修复：监控 API 现在复用服务端自启计时，不再由打开 App 的状态请求启动独立计时；自启计时也会在该卡矿工运行时清零。新增 Rust 回归测试。2-titan GPU1 的 systemd 启动时间为 13:07:59，截图时间为 13:07，说明矿工在截图后已自动启动，原有 App 倒计时早于真实策略计时。客户端 CPU 行改为明确显示“主机使用率”和“线程比例”；release APK 已构建并安装到已连接的 Redmi 设备。格式检查、25 项 Rust 测试、Clippy、debug/release 构建、9 项 Flutter 测试及 Flutter analyze 均通过。
- 本次 API binary（SHA-256 `003be8d68741837660fe244c3c36bc4702993c8eeeb850fb7a6d8b444fb83478`）部署到两台主机，API 均 active 且未认证请求返回 401。回滚副本为 `/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-idle-display-20261103T134418` 和 `/opt/gpu-pocket/gpu-pocket-server.pre-idle-display-20261103T134418`。API 重启没有重启矿工；zww 两个 GPU unit 在部署前已分别于 13:43:09 和 13:43:45 停止，当前仍 inactive。按用户确认，zww CPU 线程比例从 80% 改为 50%，CPU unit 于 13:47:52 重启后保持 active；随后日志仍报告未连接矿池，没有观察到有效挖矿负载。network namespace 至配置端口的 TCP 可达，仍需排查矿池握手/认证或代理规则。2-titan CPU 矿工以 50% 线程比例运行，观察到约 1976% 单进程 CPU（约占其 40 个逻辑 CPU 的一半）。
- 2026-11-03 zww CPU 矿池故障定位与修复：GPU0 的 `gpu-pocket-miner@0.service` 已在 `research-tun` namespace 收到 Kryptex Pearl job 并接受 shares；CPU unit 则是旧版主机网络配置，主机 DNS 解析失败。已将原 CPU unit 备份到 `/var/backups/gpu-pocket/gpu-pocket-cpu.service.pre-netns-20261103T140232`，修正为先进入 `research-tun` 再以 `zww` 运行矿工，执行 daemon-reload 并重启。14:02:44 CPU miner 成功连接 `xmr-us.kryptex.network:7029`、接收 RandomX jobs；14:05:24 出现 CPU share accepted，14:05:35 算力约 6847 H/s，配置线程比例仍为 50%。初期有两次短暂重连，随后已接受 share；GPU unit 未重启或修改。

- 2026-11-03 19:04 空闲自启修复：发现 `policy_tick` 在每轮检查外部进程时无条件清空 `low_util_since`，因此即使 GPU 持续空闲，计时也会每 5 秒重新开始，无法达到 10 分钟。已改为仅在外部任务、进程归属检查失败、GPU 指标缺失或其他不满足空闲条件时清零，并按用户要求将利用率阈值从低于 5% 改为低于 15%。自动接管仍要求没有其他计算进程，并保留显存保护。
- 新增策略循环回归测试，先复现计时被重置的失败，再验证跨轮保留计时、14.99% 允许计时、15% 和指标缺失清零、满 10 分钟自动启动及运行后清零。27 项 Rust 测试、Clippy、release 构建与 Flutter analyze 通过。两台 API 已部署相同 SHA-256 `50d933f03c047d2ef45e76e173eb7eab74099da04848eb9238bf0d46f1dff529`，服务均 active，未认证请求返回 401；部署前后 GPU/CPU units 的 ActiveState、MainPID 和 ActiveEnterTimestamp 一致。回滚副本为 `/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-idle15-20261103T190345` 与 `/opt/gpu-pocket/gpu-pocket-server.pre-idle15-20261103T190411`。
- 现场只读检查发现：2-titan GPU1 利用率约 1%，但仍有外部 Python 计算进程 PID 2571057，因此不计时符合现有保护规则；zww 也存在 Python 和 CarlaUE4 外部计算进程。没有停止这些任务。本次以模拟指标和时间验证真实策略循环，未完成生产环境连续空闲 10 分钟的实测。手机端阈值文案和说明文档已同步为 15%，本轮未构建或安装新 APK；计时修复与阈值在服务器生效，无需新 APK。

- 2026-11-03 19:09 用户明确要求排除 Chrome、Sunlogin 等常用软件的进程干扰，同时保护用户启动的 Python 训练、CARLA 模拟等计算程序。现有忽略名单包含 Sunlogin、Chrome、Chromium、Google Chrome 和 Cursor，Xorg 另行排除；增加 CARLA 不在忽略名单的回归断言。忽略进程不等于从整卡利用率中扣除其负载，空闲自启仍要求整卡利用率低于 15%。发现深夜模式原先绕过外部进程检查，已取消该例外：深夜自动运行也会检查外部计算进程，遇到外部任务或无法确认进程归属时让出 GPU，只停止受管矿工，不停止用户程序。新增真实策略循环的模拟进程检查入口，回归测试先复现绕过检查，再验证深夜矿工让出及持续阻止自启。
- 本轮 28 项 Rust 测试、Clippy 和 release 构建通过。两台 API 已部署相同 SHA-256 `e1763036040cdd92623301b6d580a318449b47c1ab592a208c0b8cf063ca049b`，服务均 active，未认证请求返回 401。GPU/CPU units 的 ActiveState、MainPID 和 ActiveEnterTimestamp 在部署前后一致。回滚副本为 `/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-user-tasks-20261103T190902` 与 `/opt/gpu-pocket/gpu-pocket-server.pre-user-tasks-20261103T190938`。README、API 和手机客户端说明已同步深夜保护规则，本轮未构建新 APK。现有手动全量启动的共享行为未调整。

- 2026-11-03 19:56 按用户要求将整卡空闲自启阈值从低于 15% 调整为低于 25%，持续时间仍为 10 分钟，常用软件忽略名单、Python/CARLA 外部任务保护及显存保护均未调整。更新服务端常量、24.99%/25% 边界与策略循环回归测试、客户端源码文案和使用说明。28 项 Rust 测试、格式检查、Clippy、release 构建通过。
- 两台 API 均已部署 SHA-256 `c010116897f8275ac59d162f3fc80ddf17c545592ea6e27154da90960570fcbd`，服务 active，未认证请求返回 401；GPU/CPU units 的 ActiveState、MainPID 和 ActiveEnterTimestamp 在部署前后一致。回滚副本为 `/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-idle25-20261103T195547` 和 `/opt/gpu-pocket/gpu-pocket-server.pre-idle25-20261103T195609`。重启 API 会重置空闲计时。本轮未构建或安装新 APK，服务器上的 25% 阈值已生效。

- 2026-11-04 本机 AI 接入：新增受现有 Bearer 鉴权保护的 AI 中转队列，服务端持久保存最近 20 条任务并支持领取、取消、离线检测和结果回传。Windows 工作进程 `tools/ai_worker.py` 主动轮询第一台服务器，使用已登录的 Codex CLI，固定从本仓库启动；只读为默认，允许修改时使用 `workspace-write`。没有为 Windows 开公网监听端口，也没有把 API 令牌写入 APK。本机已启动后台工作进程，并在当前用户启动目录创建 `GPU Pocket AI Worker.lnk`，登录 Windows 后自动启动。详细操作见 `docs/AI.md`。
- 用户明确要求 Cursor 复用桌面插件，不采用 Cursor CLI，随后要求用 GitHub 的 REA 项目分析混淆文件。已停止 Cursor CLI 登录流程，使用 `rea-agents@6.4.0`，结合对副本的 webcrack 去混淆，恢复并分析 `jg.cursor-free 2.12.5` 的业务模块。插件主要处理授权、额度、路由和应用补丁，在本轮范围内未发现直接的外部聊天接口；完整结构分析成功，但动态语义关系仍有未知项。Cursor 核心存在 Desktop Bridge 代码，当前功能开关关闭，未修改。App 保留 Cursor 桌面选项并明确显示不可用，不把它误报为已接通。证据与边界见 `docs/CURSOR_PLUGIN_ANALYSIS.md`；原始插件没有修改，私有分析产物位于 `.local/rea-*.json` 和 `.local/cursor-plugin-deobfuscated/`。
- 本轮验证：30 项 Rust 测试、格式检查、Clippy、release 构建、11 项 Flutter 测试、Flutter analyze 和 2 项本机工作进程测试通过。真实只读任务返回 `GPU_POCKET_CODEX_OK`；真实允许修改任务创建指定的测试文件并写入 `GPU_POCKET_WRITE_OK`，验证后已删除该文件。两台 API 均已部署 SHA-256 `d13f98f5f6e6c7294fc452ef191613dde82bbc377dcd9b1bea2237dfb094bcbd`，API active，未认证 AI 请求返回 401，部署前后 GPU/CPU units 的 ActiveState、MainPID 与 ActiveEnterTimestamp 一致。回滚副本为 `/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-ai-20261104T012648` 和 `/opt/gpu-pocket/gpu-pocket-server.pre-ai-20261104T013339`。
- 新版 Firebase-enabled release APK 已构建，位置为 `mobile/build/app/outputs/flutter-apk/app-release.apk`，大小 18864283 字节。本轮尝试安装时 ADB 报告 `no devices/emulators found`，因此尚未安装到手机；覆盖安装后会保留原有服务器安全存储。当前 Windows 工作进程只连接第一台服务器，不会同时领取 zww 的独立队列。

- 2026-11-04 02:03 用户重新连接 Redmi 2407FRK8EC 并完成 ADB 授权后，已使用 `adb install -r` 覆盖安装本轮 AI release APK，ADB 返回 `Success`；随后通过 MainActivity 启动 App。设备记录 `lastUpdateTime=2026-11-04 02:03:20`，版本仍为 `0.1.0`、versionCode 1。覆盖安装保留原有应用数据和服务器安全存储配置。

- 2026-11-04 02:53 新增独立的手动深夜模式：监控页每张 GPU 卡片新增“手动深夜（不避让）”按钮，使用持久化模式 `manual_night`，按 SRBMiner Pearl 强度 31 启动。该模式不受每卡自动管理开关、25% 空闲条件、10 分钟等待或 01:00–08:00 时段限制，不避让 Python/CARLA 等外部任务；管理员全局控制开关仍需允许控制。启动前检查最低可用显存，运行中即使自动管理关闭也保留显存保护；显存不足、指标缺失、GPU 消失或整次 nvidia-smi 采样失败时停止受管矿工。保护停矿后保留手动模式选择，不自行重启，用户可手动重启或退出手动模式。手动退出后交回原有自动策略。按钮确认框说明资源争用和轮询显存保护的限制，运行状态用蓝色表示。详见 `docs/MANUAL_NIGHT.md`。
- 自动指令执行前在操作锁内重新检查模式，防止手动接管前已经排队的自动停矿或重启覆盖人工选择；显存保护使用独立停矿标记。回归测试先复现排队自动停矿误停手动会话，再验证修复。32 项 Rust 测试、格式检查、Clippy、release 构建、14 项 Flutter 测试与 Flutter analyze 通过。没有在生产训练或模拟任务旁强制启动矿工做测试。
- 两台 API 均已部署最终 SHA-256 `bdbacd10fcbb4a522e5562af3538bef943f09c0504c83437f093f3f4c72f93e5`，服务 active，能力接口返回 `manual_night_supported: true`。各次 API 更新前后 GPU/CPU units 的 ActiveState、MainPID 和 ActiveEnterTimestamp 一致。最后一次更新的回滚副本为 `/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-manual-night-priority-20261104T025248` 和 `/opt/gpu-pocket/gpu-pocket-server.pre-manual-night-priority-20261104T025302`。完整功能上线前的旧 AI 版备份为 `/media/new_data3/.local/share/gpu-pocket/gpu-pocket-server.pre-manual-night-20261104T023721` 和 `/opt/gpu-pocket/gpu-pocket-server.pre-manual-night-20261104T023735`；回到不支持新模式的旧版前必须先退出手动模式，不能只停止 unit。
- 手机新版 release APK 大小 18864363 字节，已通过 `adb install -r` 覆盖安装至 Redmi 2407FRK8EC 并发送启动 MainActivity 的命令；设备记录 `lastUpdateTime=2026-11-04 02:44:12`。安装保留原有连接配置。后续读取 UI 层级时手机处于锁屏/AOD，未进行手动模式的实机点击验证；界面交互由 Flutter 测试覆盖，启动与保护逻辑由模拟矿工回归覆盖。部署后 zww 两张 GPU 仍为 inactive、模式 normal，等待用户确认开启手动模式。

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

- 继续前运行 `git status --short`。本机工作区曾包含敏感的本地凭据文件及远端工具/截图等未跟踪文件；这些文件不得暂存或提交。提交时只选择经过检查的源码、文档和依赖锁文件。
- 保留其他协作者和先前会话的改动；不要使用 reset、checkout/restore 或宽泛清理命令。
- Rust API catalog 增加 `miner` 是为手机识别 SRBMiner 能力；若新 APK 连到尚未更新的 API，Slider 仍不会显示。发布客户端之前确认两台服务器 API binary 均包含该字段并通过 `GET /api/v1/catalog` 核对；升级服务前备份 binary、确认 unit 名称，升级后检查 API 和矿工 unit。
- 自动化策略仍是自启（每张 GPU 利用率低于 25% 持续 10 分钟后按全量强度 31 启动）和深夜模式；手动 SRBMiner 强度不属于自动策略。改强度需通过 miner action 重启应用。

## 8. 凭据与隐私要求

- 不要把完整 FCM Token、Firebase API Key、服务账号 JSON/私钥、API 令牌、代理订阅地址或完整代理节点配置写入文档、日志或提交。
- 排查 Token 时只记录是否存在和数量；记录错误时保留必要异常类别，但先检查异常消息中是否包含凭据。
- 服务器端 FCM 失效 Token 错误信息不应包含 Token 原值。
- 生产配置、systemd drop-in、服务账号和运行状态属于部署侧数据；修改前备份，避免把服务器机密拷入仓库。
