# Android 构建与真机验证

客户端优先面向 Redmi K70 Ultra，使用 ARM64 APK。源码支持在连接页修改服务器地址；构建参数可以预填公开 HTTPS 地址，但不得把 API 令牌编译进 APK。首次连接时输入令牌，客户端通过 Android 安全存储保存凭据。

## 开发环境

本地 Flutter SDK 位于 `.tools/flutter`，Android SDK 位于 `.tools/android-sdk`，Gradle 缓存位于 `.tools/gradle`。这些目录均被 Git 忽略。使用 JDK 17 或更新版本；本机现有 PyCharm 的 `jbr` 可以复用，无需修改系统默认 Java。

Android 工程当前使用 Android SDK 36、Build Tools 36.0.0、NDK 28.2.13676358 和 Gradle 9.3.1。Gradle Wrapper 验证官方 SHA256。SDK 工具安装后，可以运行：

```powershell
$env:JAVA_HOME = '你的 JDK 目录'
$sdk = (Resolve-Path .tools/android-sdk).Path
& .tools/android-sdk/cmdline-tools/latest/bin/sdkmanager.bat "--sdk_root=$sdk" --licenses
& .tools/android-sdk/cmdline-tools/latest/bin/sdkmanager.bat "--sdk_root=$sdk" 'platform-tools' 'platforms;android-36' 'build-tools;36.0.0' 'ndk;28.2.13676358'
```

## 打包

在项目根目录运行，令牌只从手机输入：

```powershell
$url = (Get-Content .local/connection.json -Raw | ConvertFrom-Json).url
./tools/build_android.ps1 -JavaHome '你的 JDK 目录' -DefaultUrl $url
```

默认输出 `mobile/build/app/outputs/flutter-apk/app-debug.apk`。这是使用调试签名的测试包；正式分发前需要配置独立发布密钥。`-Mode release` 仍沿用工程当前的调试签名，不能视为正式发布签名。

构建脚本仅设置当前进程的环境变量。它关闭 Flutter 遥测，避免本机遥测初始化等待；Flutter 与 Pub 下载使用 Flutter 中国社区镜像。

## 安装和验证

可以把 APK 传到手机后安装，也可以打开 USB 调试、连接电脑并允许这台电脑调试，然后运行：

```powershell
./.tools/android-sdk/platform-tools/adb.exe devices
./.tools/android-sdk/platform-tools/adb.exe install -r mobile/build/app/outputs/flutter-apk/app-debug.apk
```

首次启动后进入连接页，核对 HTTPS 地址并输入私有连接文件中的令牌。不要发送令牌截图。Quick Tunnel 重启后地址可能变化，此时重新导出连接信息并在 App 内修改地址。

真机验收需要分别使用校园 Wi-Fi 和关闭 Wi-Fi、VPN 后的蜂窝网络，确认两张 GPU 的数据会更新。退出并重开 App 后应自动恢复连接；切到后台后停止轮询，回到前台后恢复。当前生产服务保持只读，矿工启停、钱包及算法修改不能作为已验收功能。

没有实际手机连接时，只能验证 APK 构建和本地测试，不能声称安全存储、蜂窝网络或系统安装流程已经通过真机验收。
