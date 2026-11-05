# API v1

所有接口均要求 `Authorization: Bearer <GPU_POCKET_TOKEN>`。包括监控接口在内，没有公开状态端点。普通 JSON 请求体上限为 4096 字节；AI 任务接口上限为 128 KiB，并另外校验提示词和结果长度。服务端不启用浏览器跨域访问；原生 Flutter 不依赖 CORS。

- `GET /api/v1/status`：返回 `demo`、采样时间戳、`gpus`、控制开关、`push_configured`、采样／状态文件错误列表，以及 `gpu_controls`、`policies` 和 `cpu_mining`。`gpu_controls` 每项含 `service`、暂存的 `desired`、运行记录 `active`、`external_pids` 和 `processes`；进程项含 PID、类型、名称、显存用量、可执行文件路径及其是否位于 `gpu_root` 下。Xorg 不显示。进程列表不可用时为 `null` 并附带错误，而不是空闲。`idle_seconds` 来自服务端自动策略计时，只在启用自启、该卡矿工未运行、利用率低于阈值且没有外部 GPU 进程时增长；打开客户端不会启动或重置计时，服务端重启会重新开始观察。`policies` 每项含策略、运行模式和自动化计时／保护状态。`cpu_mining` 含 CPU 专属 `service`、`desired`、`active`、`mode`、`policy`、`runtime` 和整机 `usage_percent`；CPU 低负载计时使用整机平均使用率。`threads_percent` 是 SRBMiner 工作线程比例，不是整机 CPU 限额。整机使用率与矿工线程比例不可直接比较；未连接矿池时矿工不会产生完整计算负载。旧的顶层 `service`、`desired`、`active` 字段继续映射 GPU0。`gpus` 为空且 `errors` 非空表示采样失败，不表示空闲。
- `GET /api/v1/catalog`：返回允许的 GPU/CPU 算法配置和对应矿池；GPU profile 提供公开的 `miner` 类型供客户端判断能力。不会返回二进制路径或代理控制器秘密。
- `PUT /api/v1/gpus/{gpu_id}/selection`：分别暂存 GPU0 或 GPU1 配置。请求包含匹配路径的 `gpu_id`、`profile_id`、`pool_id`、`wallet` 和 `revision`。旧版 `power_limit_w` 字段只为兼容接受并被忽略，不会设置功耗上限。revision 必须等于该 GPU 当前暂存版本；首次为 0。服务端保存后加 1。此操作不重启矿工。
- `POST /api/v1/gpus/{gpu_id}/miner`：请求为 `{"operation":"restart","revision":1,"mode":"normal"}`。`mode` 可选 `normal`（默认，全量模式）或 `throttle`。手动节制启动当前已停用；请求 `mode:"throttle"` 返回 410。该模式停用不会停止已运行的矿工。SRBMiner Pearl 全量固定使用算法强度 31；算法强度不是 GPU 利用率或功耗上限。手动全量启动允许与 GPU 上其他计算进程共存，不会停止或接管它们；显存不足或资源争用仍可能导致任一任务失败。自动启动仍要求目标 GPU 没有非 GPU Pocket 任务。operation 仅支持 `start`、`restart`、`stop`。revision 必须匹配，避免多台手机覆盖操作。
- `PUT /api/v1/cpu/selection`：暂存 CPU `profile_id`、`pool_id`、`wallet`、`threads_percent` 与 `revision`。线程比例限制为 1–100，首次 revision 为 0；未传比例时默认为 50。修改不影响 GPU 选择，也不会立即启动矿工。
- `GET /api/v1/cpu/policy`、`PUT /api/v1/cpu/policy`：读取／保存 CPU 自动策略 `enabled`、`idle_start_enabled`、`night_mode_enabled`、`idle_threshold_percent` 和 `idle_duration_minutes`。默认全部关闭，低负载阈值默认为整机 25% 且持续 10 分钟；自动深夜模式按北京时间 01:00 启动、08:00 停止。
- `POST /api/v1/cpu/miner`：以 `{"operation":"start","revision":1}` 启动独立 `gpu-pocket-cpu.service`；同样支持 `restart`、`stop`。`mode:"manual_night"` 可在北京时间 01:00–08:00 手动启动深夜模式，该模式到 08:00 自动停止，即使自动控制开关关闭；`mode:"normal"` 退出。服务端只使用 TOML 白名单中的 CPU 算法、矿池和二进制，不接收任意命令。SRBMiner 的线程比例是 worker-thread hint，不保证为操作系统级硬 CPU 限额。
- `GET /api/v1/gpus/{gpu_id}/policy`：读取该卡自动策略和当前运行模式。
- `PUT /api/v1/gpus/{gpu_id}/policy`：写入该卡自动策略（字段为 `gpu_id`、`enabled`、`throttle_enabled`、`night_mode_enabled`、`external_task_monitor`、`min_free_memory_mib`、`intensity`）。`intensity` 仅为兼容旧配置保留，不再用于启动；旧版 `power_limit_w` 被接受并忽略。深夜模式只支持 SRBMiner Pearl。全局 `control_enabled` 和每卡 `enabled` 都必须打开自动控制才会运行。自动策略仅包含空闲自启与深夜模式：每张卡在利用率低于 25% 持续 10 分钟且没有其他计算进程时，按 SRBMiner 全量强度 31 启动；这不是功耗或算力上限，启动后可能占满该卡。深夜时间为北京时间 01:00–08:00，期间仍会避让其他计算进程，并按显存阈值保护。可用显存低于阈值或无法采样时自动暂停；默认每 5 秒检查一次，该阈值是轮询停矿保护，不是显存硬上限。
- `PUT /api/v1/push/token`、`DELETE /api/v1/push/token`：注册／注销手机 Firebase Messaging 设备令牌。接口可在 Firebase 服务端凭据未配置时登记令牌；此时 `push_configured` 为 false，通知不会投递。
- 旧 `PUT /api/v1/selection` 与 `POST /api/v1/miner` 路由保留为 GPU0 兼容入口。
- `GET /api/v1/proxy`：返回固定代理组的类型、当前节点、成员和 `selectable`。未配置控制器秘密时返回 503。
- `PUT /api/v1/proxy`：请求为 `{"name":"节点名称"}`。仅允许固定 Selector 组内现有成员，拒绝 `DIRECT`，不修改整个代理配置，不调用通用配置更新接口。

错误使用 JSON `{"error":"..."}`。缺少或错误的令牌返回 401，控制关闭返回 403，参数错误返回 400，版本冲突、全量启动时目标 GPU 被非对应实例管理的进程占用或非 Selector 组返回 409。服务执行、采样依赖和控制器不可用等错误返回 503。Axum 自身的 JSON 类型或解析错误可能返回文本 4xx；手机会显示响应格式错误及连接提示。

服务端自动控制默认关闭。启用策略会启动或停止目标卡的 GPU Pocket 矿工；节制／日间提醒检测到外部进程时不会杀掉外部进程，只停止受管矿工。深夜模式也会避让外部计算进程。独立进程监视器每 5 秒检查 GPU 进程；当进程可执行文件不在 `gpu_root` 下（或路径无法确认）时，对新出现的进程发送一次推送，进程消失后再次出现可重新告警。服务启动时的首轮检测只建立基线，不为已在运行的进程发送“新进程”告警。FCM 需要管理员在服务端安全配置服务账号文件，并在 Android 构建时提供 Firebase 客户端参数；不得将服务账号 JSON 或其私钥放进仓库或 APK。

矿工命令由服务端固定适配器构造，不经过 shell。GPU Pearl 与 CPU 配置使用管理员 TOML 中的 SRBMiner 配置；OCTOPUS 使用 lolMiner。算法、二进制路径和矿池来自管理员维护的 TOML；手机不能修改这些路径或传入任意参数。钱包只接受长度 1–256 的 ASCII 字母、数字和 `._:-`，且不能以 `-` 开头。钱包合法字符不代表地址在链上有效，使用者仍需核实币种与矿池格式。

`active` 是启动时记录的配置，只在专用服务 active 时显示。它不是矿池成功连接、有效算力或已接受份额的证明。矿工运行健康仍应结合 systemd 日志与矿工 API 判断。

GPU0 磁盘状态使用 `desired.json`、`applied.json`、`active.json`；GPU1 使用 `gpu1-desired.json`、`gpu1-applied.json`、`gpu1-active.json`；CPU 使用 `cpu-desired.json`、`cpu-applied.json`、`cpu-active.json`。矿工进程及 systemd 自动恢复只读取对应的 applied，不会因用户暂存了新钱包而在之后的崩溃恢复中自动采用该钱包。`active` 仅证明服务启动时采用了该配置，不证明矿池有有效份额。

## 手动深夜模式

手动深夜控制的 `manual_night` 模式、能力标记与退出规则见 [手动深夜模式](MANUAL_NIGHT.md)。该模式使用既有 GPU miner action 接口，与 AI 任务队列无关。

## 本机 AI 任务接口

以下接口复用现有 Bearer 鉴权。API 只中转任务，本机 Windows 工作进程负责实际调用 AI；目前已连接 Codex，Cursor 桌面接入仍不可用。

- `GET /api/v1/ai`：返回固定工作目录、Codex/Cursor 可用状态和最近 20 条任务。
- `POST /api/v1/ai/tasks`：提交 `{provider, prompt, allow_edits}`。`provider` 为 `codex` 或 `cursor`，提供方必须在线可用；`allow_edits` 默认 `false`；`prompt` 最多 8000 UTF-8 字节。队列最多保留 4 条未结束任务。不接受额外命令或工作目录字段。
- `POST /api/v1/ai/tasks/{id}/cancel`：取消排队任务，或请求本机停止正在运行的任务。不会回滚已经完成的文件修改。
- `POST /api/v1/ai/worker/poll`：本机使用 `{worker_id, providers, running_task_id}` 报告心跳并串行领取任务；同一领取在响应丢失后可重领，其他在线工作进程不能抢占。
- `POST /api/v1/ai/worker/result`：本机回报 `{worker_id, id, status, output}`；最终状态为 `completed`、`failed` 或 `cancelled`，结果最多 24000 UTF-8 字节。

工作进程心跳有效期为 60 秒。任务历史以 `ai-tasks.json` 保存在现有状态目录；API 重启会将运行中的任务标记为中断。部署及本机启动见 [AI.md](AI.md)。
