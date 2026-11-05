# GPU Pocket 停用记录

日期：2026-11-06，Asia/Shanghai。

## 用户要求与执行顺序

用户要求把现有项目内容上传 GitHub，然后停止 App 的全部功能，包括 2-titan 和 zww 上的部署。本次不再继续开发此前的手动深夜模式需求。

当前阶段：源码及文档已准备归档，尚待推送成功后执行停用。停用完成后会更新本文件，记录核验结果。

## 停用范围

- 2-titan：用户级 API、Quick Tunnel；系统级 GPU0/GPU1 矿工、CPU 矿工、历史 IPv6 诊断服务。
- zww：系统级 API、GPU0/GPU1 矿工、CPU 矿工；用户级两个 GPU Pocket Quick Tunnel 服务。
- Windows：GPU Pocket AI Worker 及其登录启动快捷方式；如存在项目专属后台进程或计划任务，一并停止并禁用。
- 服务器配置：关闭全局控制与 CPU/GPU 自动策略，退出手动模式。保留配置与状态文件，以便审计或未来明确恢复。

只停止 GPU Pocket 管理的服务。现有 Python、vLLM、CARLA 任务、SSH、Tailscale 和 research-tun 不属于停用范围。

手机目前未连接 ADB，无法通过电脑强行停止手机端进程或卸载应用。服务端关闭后，手机端监控、远程矿工控制及 AI 中转将不可用。保留手机 App 和本地数据。

## 备份与隐私

源码、测试、部署脚本及文档提交到原仓库 `joshuaburgen/gpu-pocket` 的 `main` 分支。已有本地提交也随此次推送归档。不把令牌、钱包配置、Firebase 凭据、AI 对话或私有运行状态上传 GitHub。

开发机私有备份目录为 `.local/shutdown-20261106/`，已被 Git 忽略，含工作区源码快照、未提交修改补丁及两台服务器配置/状态归档。服务器端备份保存在各自用户的 `~/.local/share/gpu-pocket/shutdown-20261106/`。这些归档可能含凭据，应保持私有。

## 恢复要求

服务停止并禁用后，不会因正常开机自动恢复。重新启用必须明确决定恢复范围，并检查服务器配置、每卡/CPU 自动策略、钱包、矿池、API 令牌及公网入口。不要直接把整个私有状态归档覆盖到生产机器。

先在仅监控、全局控制关闭的状态下恢复 API，核验服务和鉴权，再按需要恢复隧道。最后才明确启用矿工控制及所需自动策略。Windows AI Worker 的登录启动需要重新运行 `tools/install_ai_worker_startup.ps1`。

本次保留二进制、systemd unit、依赖工具和私有配置，不删除项目目录，不卸载 Codex/Cursor，不修改共享网络环境。
