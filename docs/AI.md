# 本机 AI 任务

手机的“AI任务”页通过第一台已连接服务器的 API 提交任务。本机 Windows 工作进程主动轮询这个 API，调用已登录的 Codex CLI，并把结果交回服务器。Windows 不监听额外端口，也不需要新建公网隧道。

## 当前能力

- **Codex**：已接通。使用本机 Codex 登录和默认模型配置；每次提交是独立任务，不会接续当前桌面 Codex 会话。
- **Cursor（桌面 Agent）**：保留选项，但当前不可提交。用户要求复用桌面插件，不采用 Cursor CLI。当前 Cursor 3.28.7 的 Desktop Bridge 功能开关为关闭，未发现可连接的桌面桥接文件；插件的具体调用能力仍在只读分析中。

选择 Codex，输入任务后点击“提交任务”。默认只读；只有开启“允许修改项目文件”时，本次 Codex 进程才使用 `workspace-write`。工作目录固定为本仓库。页面每 5 秒刷新状态，支持取消和查看最终结果。电脑必须保持开机，且本机工作进程需要能访问第一台服务器。

常用软件进程过滤、GPU 自动策略和 AI 任务队列相互独立。服务器只保存和转发 AI 任务，不执行模型命令。任务描述中的项目操作仍会交给本机 AI 处理，请明确描述需要修改的范围。

## 本机启动

本机需要 Python 3.10 或更新版本、已登录的 Codex CLI，以及私有的 `.local/connection.json`。该文件与 App 第一台服务器使用同一个 HTTPS 地址和令牌，不应提交或放入 APK。

```powershell
python tools/ai_worker.py --check
./tools/start_ai_worker.ps1
```

后台日志保存在 `.local/ai-worker/stdout.log` 和 `stderr.log`。日志不记录令牌、任务提示词或账号详情。需要诊断时可在终端前台运行：

```powershell
python -X utf8 -u tools/ai_worker.py
```

以下脚本在当前 Windows 用户的启动目录创建快捷方式，登录 Windows 后自动启动工作进程：

```powershell
./tools/install_ai_worker_startup.ps1
```

要关闭自动启动，删除用户启动目录中的 `GPU Pocket AI Worker.lnk`。关闭电脑或退出工作进程后，App 会在 60 秒内把本机 AI 显示为离线。

## 队列与中断

API 使用现有 Bearer 鉴权。任务历史保存在服务器状态目录的 `ai-tasks.json`，最多保留 20 条任务；最多同时排队和运行 4 条任务。本机串行执行，任务最长运行 30 分钟，返回结果最多 24000 UTF-8 字节。

运行中任务每约 3 秒检查取消请求；中转连接持续中断超过 40 秒时终止本机 CLI。服务器 API 重启会把运行中任务标记为中断；本机不会自动重试可能已经修改项目的任务。取消不会撤销此前已经完成的文件修改。

协议只接收提供方、任务文本和是否允许修改，不接受调用者指定命令、程序路径或工作目录。Codex 使用其本身的沙箱；这些约束不应被解释为对自然语言任务内容的形式化安全证明。

## 验证

服务端测试覆盖鉴权、离线提供方、领取与重领、串行执行、取消、历史保存、队列上限及拒绝工作目录注入。手机测试覆盖只读提交、取消和 Cursor 不可用提示。本机测试覆盖沙箱参数、独立进程返回以及取消时终止子进程。

2026-11-04 已完成两次真实中转调用：只读任务返回 `GPU_POCKET_CODEX_OK`；允许修改的任务创建了指定的 `artifacts/ai-bridge-smoke.txt` 并写入 `GPU_POCKET_WRITE_OK`，验证后该测试文件已删除。这些结果证明 Codex 的问答和项目写入链路可用，不代表 Cursor 已接通。Cursor 的 REA 调查见 [插件分析报告](CURSOR_PLUGIN_ANALYSIS.md)。

官方接入资料：[Codex 非交互模式](https://learn.chatgpt.com/docs/non-interactive-mode)、[Cursor Deeplinks](https://cursor.com/docs/reference/deeplinks)。Cursor 的 Deeplink 只能预填提示词，需要在电脑确认发送，不能直接代替手机端的完整任务接口。
