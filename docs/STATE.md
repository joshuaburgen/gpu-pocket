# 开发交接状态

日期：2026-10-31，Asia/Shanghai。

Android 进度：已在本机安装项目内的 JDK 17 和 Android SDK 36，并构建 ARM64 debug APK。构建信息及真机验证状态见下方“验证证据”。

逐卡控制和自动策略已进入代码并通过开发环境验证：服务端支持 GPU0/GPU1 独立策略、节制／深夜调度、外部进程保护、显存余量停矿及 FCM 设备注册；手机端提供对应策略表单和推送初始化。生产 API 仍保持 `control_enabled = false`，没有远程部署、启动或停止矿工。Firebase 服务账号／客户端参数尚未配置，因此推送没有实测；真实矿工逐卡功耗限制也没有实测。

## 当前部署

用户确认另一张 SIM 没有可用流量，跳过双运营商对照。已将现有测试的精确时间、源地址、目标地址/端口、抓包统计及待核实事项整理到本地私有 `.local/ipv6-network-admin-request.md`，尚未发送给任何人。具体过滤节点仍未确定，后续需要网络运营方日志或新的独立路径证据。

22:37 热点对照更新：电脑连接手机热点后获得原生 240e: 前缀 IPv6，原生 socket 能连清华 IPv6-only TCP 443，但不能连接服务器两个 IPv6 地址的 SSH 99，主动 ping 服务器也失败。服务器主动 ping 手机成功并能抓到回复；同期未抓到热点电脑发起的 SYN/Echo Request。手机浏览器已不再是唯一候选原因，但具体过滤节点仍未确定。Quick Tunnel 在热点网络下鉴权和 GPU 读取测试通过。下一步可更换另一家运营商的热点对照。

22:08 更新：手机在蜂窝网络可以访问清华 IPv6-only 站点；服务器也能通过 IPv6 访问清华和腾讯目标。同步抓包显示校园客户端连接可达但存在重传和延迟。外部证书机构探测的精确时间窗内未抓到目标 TCP 80 数据包，仍不能把原因确定为校园网防火墙。诊断入口和抓包已停止，Quick Tunnel/API 正常。需手机蜂窝运营商及 IPv6 来源地址继续定位。汇总见 docs/ipv6-capture-summary.json。

21:58 同步抓包更新：校园网客户端的请求确实到达服务器，服务器已发出 SYN-ACK 和 HTTP 响应，但存在多次重传及延迟确认；用户随后确认校园 Wi-Fi 等待后可打开。不能继续笼统归因于入站拦截。蜂窝网络仍未定位，正在通过独立 IPv6-only 站点确认手机蜂窝 IPv6 本身。详见 IPV6.md 的同步抓包修正。

IPv6 校外测试最终结果：用户确认手机关闭 Wi-Fi/VPN、使用 4G/5G 访问 HTTP 80 测试页超时或打不开。结合证书机构对 80/443 的连接失败，当前没有验证可用的校外 IPv6 直连路径。诊断服务已停止并禁用，配置保留，App 继续通过 Quick Tunnel 访问。若继续 IPv6 方案，需要校园网管理员核实边界入站和路由。

IPv6 后续验证：电脑已具备公网 IPv6，校内 SSH 99、HTTP 8766/80 均可达；用户确认手机成功的网络为校园 Wi-Fi，校外连通性尚待蜂窝测试。Let's Encrypt 测试环境对该 IPv6 的 HTTP-01（80）与 TLS-ALPN-01（443）均连接失败，未启用生产 HTTPS。当前新增的 gpu-pocket-ipv6.service 仅提供 HTTP 80 测试和 ACME 路由；443 测试已撤下，8766 临时服务已停止。详细证据和下一步见 IPV6.md，App 继续使用 Quick Tunnel。

17:28 更新：按用户选择使用免费 Cloudflare Quick Tunnel。主机 DNS 已修复，用户级只读 API 和隧道均已启动并启用，joshua 的 linger 已开启。当前地址为 `https://federation-privileges-honey-submitting.trycloudflare.com`。连接令牌仅存于被 Git 忽略的本地 `.local/connection.json` 和远端私有配置目录，不写入本文件。详细管理步骤见 QUICK_TUNNEL.md。

开发机与服务器均已完成公网 HTTPS 验证：匿名返回 401、正确令牌返回 200、矿工控制返回 403，两张真实 GPU 均可读取且采样无错误。原有矿工没有被接管或停止。

## 已完成

项目位于 `D:\work\github\gpu-pocket`，已初始化本地 Git 仓库，尚未创建 GitHub 远程仓库或推送。现有其他项目没有被改动。

Rust 服务端已完成 GPU 指标采集、全路由 Bearer 鉴权、算法与矿池目录、钱包暂存、版本冲突检查、GPU0 专用服务控制、Mihomo 指定代理组适配，以及显式演示模式。运行状态分为暂存、明确应用和启动记录，防止未应用的钱包随崩溃恢复意外生效。手机不能提交 shell、二进制路径或任意矿工参数。

Flutter 端包含监控、控制和连接三个页面，提供自动刷新、后台停止轮询、采样失败提示、操作确认、HTTPS 校验、系统安全存储及本机凭据删除。Android / iOS 原生工程已生成。Android 已加互联网权限、回环明文例外与关闭备份配置；iOS 已加入安全存储要求的 Keychain entitlement。

部署目录提供只读 API unit、GPU0/GPU1 共用的 miner systemd 模板、限定到两张卡实例的 sudoers、Cloudflare Tunnel 和 IPv6 Caddy 示例。当前实际运行的是 tools/install_quick.py 安装的用户级只读服务，尚未安装矿工 unit 或 sudoers。GitHub Actions 检查工作流已写入，尚未在 GitHub 运行。

逐卡自动策略需全局 `control_enabled = true` 并在 App 中逐卡启用；所有策略默认关闭。节制和深夜模式目前只支持 Pearl/SRBMiner。深夜模式忽略功耗／强度、计算占用和外部进程；显存通过 5 秒轮询阈值触发停矿，不是硬上限。节制／日间提醒检测外部任务时不会停止外部进程，只暂停 GPU Pocket 管理的矿工。推送依赖 Firebase 项目及服务账号配置。

## 验证证据

- Flutter 3.47.9 / Dart 3.13.7：逐卡改动后 `dart format --output=none --set-exit-if-changed lib test`、`flutter analyze` 无问题、3 项测试通过。
- Rust 1.99.1，Linux x86_64：逐卡改动后 `cargo fmt -- --check`、7 项 `cargo test`、`cargo build` 和 `cargo clippy --all-targets -- -D warnings` 均通过。
- 自动策略及推送改动后：远端 Rust `cargo fmt -- --check`、11 项 `cargo test --locked`、`cargo clippy --all-targets --locked -- -D warnings` 和 `cargo build --locked` 全部通过；新增北京时间边界、策略默认关闭、推送令牌注册与删除、demo 不查询主机 GPU 进程测试。
- 自动策略及推送改动后：Flutter format、analyze、3 项测试通过；ARM64 debug APK 构建成功，`mobile/build/app/outputs/flutter-apk/app-debug.apk`（109,353,015 字节）。包名 `com.tongjiwang.gpu_pocket`、最低 SDK 24、目标 SDK 36；Firebase define 未提供，所以此构建未启用推送。
- `tools/smoke.py` 演示模式 HTTP 测试通过，覆盖鉴权、暂存、冲突拒绝、应用、代理节点选择与停止。
- 同一脚本的真实只读模式测试通过，从 `2-titan` 的实际 `nvidia-smi` 获得两张 RTX 3080 Ti，采样错误为空；实际写接口均验证为 403。测试服务位于随机回环端口，结束后已退出。
- Python 工具通过语法编译检查。
- 当前没有 Android 设备连接到 ADB，尚未安装或真机验证；iOS 未在 macOS / Xcode 编译或真机验证，安全存储的真实设备持久化也尚未验证。

真实矿工启停、算法／钱包切换及 Mihomo 写操作尚未在生产环境执行。自动测试中的写入只作用于临时演示服务。不要将接口实现与测试通过理解为生产接管已完成。

## 网络结论

已采用 Cloudflare Quick Tunnel，并通过 HTTP/2 连接到香港节点。主机 DNS 故障来自无线接口运行时上游被覆盖为没有本地接管的代理虚拟地址 198.18.0.2；恢复学校 DHCP DNS 202.120.190.208、202.120.190.108 后验证成功。未确定具体写入覆盖值的进程和时间。修复没有重启网络、修改 NetworkManager 连接配置或改动 research-tun。备份和回滚位于 /var/backups/gpu-pocket-dns/20261030-172126-983516/。IPv6 外部入站仍未验证。

当前代理控制器要求授权，`RESEARCH_AUTO` 是 Fallback 自动组。App 对该组仅展示自动选择状态；手选需要管理员建立并验证专用 Selector 组。

## 待落地事项

1. ADB 当前未列出手机；启用 USB 调试、授权电脑后再安装 APK 并验证真机连接、安全存储及通知权限。
2. 需要 Firebase 客户端参数和服务端服务账号后重新构建 APK，并测试实际 FCM 通知；推送凭据不得进入 Git。
3. 生产 API 仍为只读。逐卡接管需审核 GPU 分配、现场人工停止旧监督器后才启用控制；真实矿工功耗参数和自动模式尚未在生产 GPU 上实测，不能未经授权自行变更现有任务。
4. 按需加入矿工算力、份额、无任务监测。现有 Python 监督器的完整保活逻辑未迁移，新矿工服务模板只有 systemd 退出重启。
5. Quick Tunnel 重启后地址可能变化，需要重新导出连接信息并更新 App。尚未重启整机验证开机启动。

## 临时环境

Flutter SDK 下载在项目 `.tools/flutter`，被 Git 忽略。远端 Rust 工具链位于 `/tmp/gpu-pocket-tools`，编译副本和调试二进制位于 `/tmp/gpu-pocket-build`。早期构建使用的临时 SSH 代理及测试 API 已退出，没有修改用户原有默认 Rust 配置。实际运行的程序现已复制到 ~/.local/share/gpu-pocket，不依赖 /tmp 编译目录。验证后的四个 Rust 源文件和 Cargo.lock 与远端 SHA256 逐项一致。
