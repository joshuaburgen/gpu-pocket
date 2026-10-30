# API v1

所有接口均要求 `Authorization: Bearer <GPU_POCKET_TOKEN>`。包括监控接口在内，没有公开状态端点。JSON 请求体上限为 4096 字节。服务端不启用浏览器跨域访问；原生 Flutter 不依赖 CORS。

- `GET /api/v1/status`：返回 `demo`、采样时间戳、`gpus`、控制开关、采样错误列表，以及 `gpu_controls` 对象（键 `"0"`、`"1"`；每项含 `service`、暂存的 `desired`、运行记录 `active`）。旧的顶层 `service`、`desired`、`active` 字段继续映射 GPU0。`gpus` 为空且 `errors` 非空表示采样失败，不表示空闲。
- `GET /api/v1/catalog`：返回允许的算法配置和对应矿池。不会返回二进制路径或代理控制器秘密。
- `PUT /api/v1/gpus/{gpu_id}/selection`：分别暂存 GPU0 或 GPU1 配置。请求包含匹配路径的 `gpu_id`、`profile_id`、`pool_id`、`wallet`、可选 `power_limit_w` 和 `revision`。功耗上限必须在该卡当前 nvidia-smi 范围内；不可用时拒绝设置。revision 必须等于该 GPU 当前暂存版本；首次为 0。服务端保存后加 1。此操作不重启矿工。
- `POST /api/v1/gpus/{gpu_id}/miner`：请求为 `{"operation":"restart","revision":1}`。operation 仅支持 `start`、`restart`、`stop`。start 对已运行的实例保持 systemd 原有行为；应用新配置应使用 restart。revision 必须匹配，避免多台手机覆盖操作。
- 旧 `PUT /api/v1/selection` 与 `POST /api/v1/miner` 路由保留为 GPU0 兼容入口。
- `GET /api/v1/proxy`：返回固定代理组的类型、当前节点、成员和 `selectable`。未配置控制器秘密时返回 503。
- `PUT /api/v1/proxy`：请求为 `{"name":"节点名称"}`。仅允许固定 Selector 组内现有成员，拒绝 `DIRECT`，不修改整个代理配置，不调用通用配置更新接口。

错误使用 JSON `{"error":"..."}`。缺少或错误的令牌返回 401，控制关闭返回 403，参数错误返回 400，版本冲突、目标 GPU 被非对应实例管理的进程占用或非 Selector 组返回 409。服务执行、采样依赖和控制器不可用等错误返回 503。Axum 自身的 JSON 类型或解析错误可能返回文本 4xx；手机会显示响应格式错误及连接提示。

矿工命令由服务端固定适配器构造，不经过 shell。算法、二进制路径和矿池来自管理员维护的 TOML；手机不能修改这些路径或传入任意参数。钱包只接受长度 1–256 的 ASCII 字母、数字和 `._:-`，且不能以 `-` 开头。钱包合法字符不代表地址在链上有效，使用者仍需核实币种与矿池格式。

`active` 是启动时记录的配置，只在专用服务 active 时显示。它不是矿池成功连接、有效算力或已接受份额的证明。矿工运行健康仍应结合 systemd 日志与矿工 API 判断。

GPU0 磁盘状态使用 `desired.json`、`applied.json`、`active.json`；GPU1 使用 `gpu1-desired.json`、`gpu1-applied.json`、`gpu1-active.json`。矿工进程及 systemd 自动恢复只读取各卡的 applied，不会因用户暂存了新钱包而在之后的崩溃恢复中自动采用该钱包。
