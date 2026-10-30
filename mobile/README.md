# GPU Pocket 手机端

Flutter 原生 Android / iOS 客户端。使用 Dart 3.10 或更新版本和当前 Flutter stable；本次验证版本为 Flutter 3.47.9 / Dart 3.13.7。安全存储使用 flutter_secure_storage 11.2.0。

运行 `flutter pub get`、`flutter analyze`、`flutter test` 后，通过 `flutter run` 启动。Android APK 构建需要 Android SDK / JDK，iOS 构建与签名需要 macOS / Xcode。Android 最低 API 23，iOS 工程目标为 15.0。iOS 已配置 Keychain entitlement，真机仍需设置自己的签名团队。

App 默认使用 HTTPS，不接受远端 HTTP 或无效证书。`http://127.0.0.1:8787` 用于调试；Android 真机／模拟器可用 `adb reverse tcp:8787 tcp:8787` 将其连接至电脑开发端口。根目录 README 说明了演示服务器与 SSH 转发方式。

监控每 5 秒刷新，进入后台停止轮询。控制页区分暂存配置与运行记录，应用操作需要确认，受服务端控制开关与版本检查约束。系统安全存储仅保存服务器 URL 与 API 令牌，不保存 SSH 密钥、矿池私钥或助记词。
