# Cursor 插件接入调查

日期：2026-11-04，Asia/Shanghai。

## 结论与适用范围

已经使用 [morluto/rea](https://github.com/morluto/rea) 的 `rea-agents@6.4.0` 对本机 `jg.cursor-free 2.12.5` 进行静态逆向分析。原始混淆入口在 60 秒和 300 秒两次分析中超时；使用 [webcrack](https://github.com/j4k0xb/webcrack) 对副本去混淆后，REA 成功完成结构分析，并对五个业务模块建立了静态语义证据。

恢复后的代码显示，该插件主要提供授权管理、额度查询、代理配置、路由及补丁模式切换。它通过修改 Cursor 配置和应用文件来影响桌面 Agent 的连接方式。在本轮已分析的插件入口和业务模块中，未发现可让外部 App 提交提示词并读取完整 AI 回复的独立聊天接口。

这个结论是有边界的静态调查结果，不是对插件所有动态行为的否定证明。服务器下载的补丁规则、WebAssembly 实现和实际桌面 Agent 的完整运行链路尚未还原，因此不能据此声称“任何方式都无法复用桌面 Cursor”。目前也不能把插件的额度或代理地址直接当作聊天 API 使用。

## 目标与产物

- 安装目录：`C:/Users/gax/.cursor/extensions/jg.cursor-free-2.12.5`。
- 主入口：`dist/extension.js`，3175664 字节，SHA-256 `5619dfc0bd7477dc158d2095c875716c3199274e05d1673fb8d6d6c05fe22677`。
- 去混淆副本：`.local/cursor-plugin-deobfuscated/deobfuscated.js`，820840 字节，SHA-256 `47585eddf1855537c84019987b797be6be8ae296dc84ab558a3ec8f355dd61c7`。
- 五个业务模块副本与来源映射：`.local/rea-plugin-business/`，其中 `source-map.json` 记录模块编号、原始行号和工厂代码摘要。
- 完整 REA 证据保存在 `.local/rea-*.json`。这些较大的分析文件被 Git 忽略，不随项目提交。

去混淆恢复的是可读代码形态，不是插件作者的原始 TypeScript 源码。业务模块从去混淆输出按工厂边界提取后包在惰性对象字面量中供 REA 分析，没有加载插件入口。webcrack 在隔离环境中求值字符串解码辅助函数；本轮没有独立启动该扩展，没有修改已安装插件，没有读取或导出运行中的账号令牌，也没有向其授权服务提交凭据。

## 证据记录

| 分析 | Evidence ID | 覆盖与限制 |
| --- | --- | --- |
| 原始目录，60 秒上限 | `ev_c8a89e559925c5d9022e9c5ed3dfd1cb325425cd33ef33e509c5d42150994b5c` | 超时，仅保留部分观察。 |
| 原始目录，300 秒上限 | `ev_f16d7643788520e822a669368616c94eee7a8b0bae8d61059f8509941196bfec` | 同样超时，不能据此推断接口不存在。 |
| 整份去混淆入口 | `ev_469719047238dd17540c3e7ea67180cfc788f1d897b543528b98ce84c2297990` | 结构覆盖完整，88566 个 AST 节点；语义图达到节点上限，保留 20000 个节点和 15178 条关系，仍有 14439 项未知。 |
| 五个业务模块 | `ev_c4b7dfd3d80af5c305f34a28f8a7011e2da94eeded0605347e0bfab8103024ce` | 结构覆盖完整，12050 个 AST 节点；语义图保留 7410 个节点和 5361 条关系，没有数量截断，但动态绑定仍产生 2658 项未知，整体语义覆盖标为 unknown。 |

结构覆盖完整只说明选定代码完成了分析，不代表所有运行关系都已证明。REA 未能把去混淆输出识别成原生 Webpack 模块图，模块身份通过惰性工厂编号、源码范围和摘要另行保留。

## 已确认的代码路径

下列行号均指去混淆副本，不是已安装原始文件的行号。

1. `package.json` 声明扩展用途为授权管理，入口为 `dist/extension.js`。注册命令包括面板、授权、代理开关、恢复 Cursor、补丁模式切换、路由切换和语言切换；没有声明聊天命令。
2. `ApiService` 工厂编号为 `1046`，从第 4173 行开始。可读方法包含 `activateKey`、`refreshKey`、`releaseDevice`、`lookupQuota`、`getNotification`、`getProxyConfig`、`getUserRole` 和 `getIpInfo`。请求由 `makeRequestWithHeaders` 统一发送。观察到的路径包括 `/api/activate`、`/api/refresh`、`/api/refresh-local`、`/api/release`、`/api/lookup` 和代理配置路径。这些是授权和管理接口，不能当作模型推理接口。
3. `ProxyService` 工厂编号为 `5118`，从第 7579 行开始。`enableProxy` 读取授权配置，调用 `getProxyConfig`，校验返回代理地址，再更新 Cursor 的 HTTP 代理设置；未创建本机聊天 HTTP 服务。
4. `CursorPatchService` 工厂编号为 `2470`，从第 4712 行开始；`applyPatchWithRules` 位于第 6195 行。入口中的模式切换操作会加载并应用版本相关补丁规则。恢复出的代码表明其主要作用对象是 Cursor 的应用文件和配置，而不是建立外部任务队列。
5. `CursorRulesPatchService` 工厂编号为 `4509`，从第 7217 行开始；`StorageService` 工厂编号为 `9965`，从第 7983 行开始。前者负责版本相关规则，后者保存插件配置和授权状态。本轮只分析代码，没有读取其运行时私有存储值。

上述判断结合了 REA 的源码范围和语义观察，以及对恢复代码的人工检查。部分动态请求目标、下载规则的执行后果和未知调用关系没有被静态证据证明。

## 与 Cursor 核心的区别

只读检查 Cursor 3.28.7 安装文件时，另外发现核心程序包含 `composer.desktopBridge.listThreads` 和 `composer.desktopBridge.sendMessage`，并通过本机发现目录 `.cursor/desktop-bridge` 发布连接信息。启动条件同时要求产品功能开关和用户开关开启。

本机存储中的 `cursor.desktopBridge.enabled` 为 `false`，发现目录不存在。这是当前桥接未启动的现场证据。它属于 Cursor 核心，不是上述插件对外提供的接口。本轮没有修改功能开关，也没有读取桥接调用令牌。

官方公开的 [Cursor Deeplink](https://cursor.com/docs/reference/deeplinks) 只保证预填提示词，仍需要用户确认发送，不提供自动读取回复的能力。因此它不能直接满足手机端的完整收发需求。

## 对 GPU Pocket 的影响

Codex 路径已经接通，并验证了只读回复和受限项目文件写入。Cursor 选项保留为不可用，明确显示桌面接口未接通；不会改用用户拒绝的 Cursor CLI，也不会把成功发送到桌面误报为收到 AI 回复。

后续若继续接入桌面 Cursor，应优先研究合法启用后 Desktop Bridge 的实际协议与回复获取方式，或找到插件提供方支持的集成接口。不能只凭域名、授权状态或静态方法名称宣称已具备完整聊天能力。
