# API v1

所有接口均要求 `Authorization: Bearer <GPU_POCKET_TOKEN>`。包括监控接口在内，没有公开状态端点。JSON 请求体上限为 4096 字节。服务端不启用浏览器跨域访问；原生 Flutter 不依赖 CORS。

- `GET /api/v1/status`：返回 `demo`、采样时间戳、`gpus`、控制开关、`push_configured`、采样／状态文件错误列表，以及 `gpu_controls`、`policies` 和 `cpu_mining`。`gpu_controls` 每项含 `service`、暂存的 `desired`、运行记录 `active`、`external_pids` 和 `processes`；进程项含 PID、类型、名称、显存用量、可执行文件路径及其是否位于 `gpu_root` 下。Xorg 不显示。进程列表不可用时为 `null` 并附带错误，而不是空闲。`policies` 每项含策略、运行模式和自动化计时／保护状态。`cpu_mining` 含 CPU 专属 `service`、`desired` 和 `active` 状态。旧的顶层 `service`、`desired`、`active` 字段继续映射 GPU0。`gpus` 为空且 `errors` 非空表示采样失败，不表示空闲。
- `GET /api/v1/catalog`：返回允许的 GPU/CPU 算法配置和对应矿池。不会返回二进制路径或代理控制器秘密。
- `PUT /api/v1/gpus/{gpu_id}/selection`：分别暂存 GPU0 或 GPU1 配置。请求包含匹配路径的 `gpu_id`、`profile_id`、`pool_id`、`wallet`、可选 `power_limit_w` 和 `revision`。功耗上限必须在该卡当前 nvidia-smi 范围内；不可用时拒绝设置。revision 必须等于该 GPU 当前暂存版本；首次为 0。服务端保存后加 1。此操作不重启矿工。
- `POST /api/v1/gpus/{gpu_id}/miner`：请求为 `{"operation":"restart","revision":1}`。operation 仅支持 `start`、`restart`、`stop`。start 对已运行的实例保持 systemd 原有行为；应用新配置应使用 restart。revision 必须匹配，避免多台手机覆盖操作。
- `PUT /api/v1/cpu/selection`：暂存 CPU `profile_id`、`pool_id`、`wallet`、`threads_percent` 与 `revision`。线程比例限制为 1–100，首次 revision 为 0；未传比例时默认为 50。修改不影响 GPU 选择，也不会立即启动矿工。
- `POST /api/v1/cpu/miner`：以 `{"operation":"start","revision":1}` 启动独立 `gpu-pocket-cpu.service`；同样支持 `restart`、`stop`。服务端只使用 TOML 白名单中的 CPU 算法、矿池和二进制，不接收任意命令。SRBMiner 的线程比例是 worker-thread hint，不保证为操作系统级硬 CPU 限额。
- `GET /api/v1/gpus/{gpu_id}/policy`：读取该卡自动策略和当前运行模式。
- `PUT /api/v1/gpus/{gpu_id}/policy`：写入该卡策略（字段为 `gpu_id`、`enabled`、`throttle_enabled`、`night_mode_enabled`、`external_task_monitor`、`min_free_memory_mib`、`power_limit_w`、`intensity`）。SRBMiner `intensity` 限制为 0–31，默认 20。全局 `control_enabled` 和每卡 `enabled` 都必须打开自动控制才会运行。自动节制／深夜目前仅支持 Pearl / SRBMiner；可用显存低于阈值或无法采样时自动暂停，显存阈值是轮询停矿保护，不是矿工硬上限。节制在利用率低于 5% 持续 30 分钟后启动，并使用设置的功耗上限和算法强度；深夜时间为北京时间 01:00–08:00，期间忽略计算占用和外部进程，只按显存阈值保护。
- `PUT /api/v1/push/token`、`DELETE /api/v1/push/token`：注册／注销手机 Firebase Messaging 设备令牌。接口可在 Firebase 服务端凭据未配置时登记令牌；此时 `push_configured` 为 false，通知不会投递。
- 旧 `PUT /api/v1/selection` 与 `POST /api/v1/miner` 路由保留为 GPU0 兼容入口。
- `GET /api/v1/proxy`：返回固定代理组的类型、当前节点、成员和 `selectable`。未配置控制器秘密时返回 503。
- `PUT /api/v1/proxy`：请求为 `{"name":"节点名称"}`。仅允许固定 Selector 组内现有成员，拒绝 `DIRECT`，不修改整个代理配置，不调用通用配置更新接口。

错误使用 JSON `{"error":"..."}`。缺少或错误的令牌返回 401，控制关闭返回 403，参数错误返回 400，版本冲突、目标 GPU 被非对应实例管理的进程占用或非 Selector 组返回 409。服务执行、采样依赖和控制器不可用等错误返回 503。Axum 自身的 JSON 类型或解析错误可能返回文本 4xx；手机会显示响应格式错误及连接提示。

服务端自动控制默认关闭。启用策略会启动或停止目标卡的 GPU Pocket 矿工；节制／日间提醒检测到外部进程时不会杀掉外部进程，只停止受管矿工。深夜模式按配置忽略外部进程。独立进程监视器每 5 秒检查 GPU 进程；当进程可执行文件不在 `gpu_root` 下（或路径无法确认）时，对新出现的进程发送一次推送，进程消失后再次出现可重新告警。服务启动时的首轮检测只建立基线，不为已在运行的进程发送“新进程”告警。FCM 需要管理员在服务端安全配置服务账号文件，并在 Android 构建时提供 Firebase 客户端参数；不得将服务账号 JSON 或其私钥放进仓库或 APK。

矿工命令由服务端固定适配器构造，不经过 shell。GPU Pearl 与 CPU 配置使用管理员 TOML 中的 SRBMiner 配置；OCTOPUS 使用 lolMiner。算法、二进制路径和矿池来自管理员维护的 TOML；手机不能修改这些路径或传入任意参数。钱包只接受长度 1–256 的 ASCII 字母、数字和 `._:-`，且不能以 `-` 开头。钱包合法字符不代表地址在链上有效，使用者仍需核实币种与矿池格式。

`active` 是启动时记录的配置，只在专用服务 active 时显示。它不是矿池成功连接、有效算力或已接受份额的证明。矿工运行健康仍应结合 systemd 日志与矿工 API 判断。

GPU0 磁盘状态使用 `desired.json`、`applied.json`、`active.json`；GPU1 使用 `gpu1-desired.json`、`gpu1-applied.json`、`gpu1-active.json`；CPU 使用 `cpu-desired.json`、`cpu-applied.json`、`cpu-active.json`。矿工进程及 systemd 自动恢复只读取对应的 applied，不会因用户暂存了新钱包而在之后的崩溃恢复中自动采用该钱包。`active` 仅证明服务启动时采用了该配置，不证明矿池有有效份额。
