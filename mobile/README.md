# GPU Pocket 手机端

Flutter 原生 Android / iOS 客户端。使用 Dart 3.10 或更新版本和当前 Flutter stable；本次验证版本为 Flutter 3.47.9 / Dart 3.13.7。安全存储使用 flutter_secure_storage 11.2.0。

运行 `flutter pub get`、`flutter analyze`、`flutter test` 后，通过 `flutter run` 启动。Android APK 构建需要 Android SDK / JDK，iOS 构建与签名需要 macOS / Xcode。Android 最低 API 23，iOS 工程目标为 15.0。iOS 已配置 Keychain entitlement，真机仍需设置自己的签名团队。

App 默认使用 HTTPS，不接受远端 HTTP 或无效证书。`http://127.0.0.1:8787` 用于调试；Android 真机／模拟器可用 `adb reverse tcp:8787 tcp:8787` 将其连接至电脑开发端口。根目录 README 说明了演示服务器与 SSH 转发方式。

监控每 15 秒刷新，编辑文本时暂停界面轮询，进入后台停止轮询。GPU 卡片列出 GPU 进程（不含 Xorg）、PID、显存用量和可执行文件路径，并标出进程是否位于 `Tongji-Wang/gpu`。前台发现新出现的 GPU 目录外进程时显示提示；后台推送要求服务器服务账号、手机 Firebase 客户端配置和通知权限均已设置。钱包预设包含已配置的 Kryptex 与 NiceHash 账户，也可选择自定义。控制卡片用绿色表示 GPU Pocket 矿工运行、黄色表示有其他进程占用、灰色表示空闲；外部进程状态无法确认时显示橙色。控制页区分暂存配置与运行记录，应用操作需要确认，受服务端控制开关与版本检查约束。CPU 挖矿位于独立控制面板，配置与服务不影响 GPU0/GPU1；线程比例默认 50%，可调 1–100%，但 SRBMiner 将其作为线程提示而非严格系统级 CPU 限额。系统安全存储仅保存服务器 URL 与 API 令牌，不保存 SSH 密钥、矿池私钥或助记词。

Pearl GPU 与 CPU 配置由服务端固定适配到管理员配置的 SRBMiner；手机只选择服务器允许的配置，不提交任意矿工命令。Krig 1.5.12 切换因服务器 glibc 不兼容而暂停，生产仍使用 SRBMiner。
