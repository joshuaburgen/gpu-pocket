import 'dart:async';

import 'package:flutter/material.dart';

import 'api.dart';

class AiTasksPanel extends StatefulWidget {
  const AiTasksPanel({
    super.key,
    required this.api,
    required this.onOpenSettings,
  });
  final GpuApi? api;
  final VoidCallback onOpenSettings;

  @override
  State<AiTasksPanel> createState() => _AiTasksPanelState();
}

class _AiTasksPanelState extends State<AiTasksPanel>
    with WidgetsBindingObserver {
  final _prompt = TextEditingController();
  Timer? _timer;
  JsonMap? _status;
  String? _error;
  String _provider = 'codex';
  bool _allowEdits = false, _working = false, _loading = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _startTimer();
    _refresh();
  }

  void _startTimer() {
    _timer?.cancel();
    _timer = Timer.periodic(const Duration(seconds: 5), (_) => _refresh());
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      _startTimer();
      _refresh();
    } else {
      _timer?.cancel();
    }
  }

  @override
  void didUpdateWidget(covariant AiTasksPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.api != widget.api) {
      _status = null;
      _error = null;
      _loading = false;
      _refresh();
    }
  }

  Future<void> _refresh() async {
    final api = widget.api;
    if (api == null || _loading) return;
    _loading = true;
    try {
      final status = await api.request('GET', '/api/v1/ai');
      if (mounted && api == widget.api) {
        setState(() {
          _status = status;
          _error = null;
        });
      }
    } catch (error) {
      if (mounted && api == widget.api) {
        setState(() => _error = 'AI 连接失败：$error');
      }
    } finally {
      if (api == widget.api) _loading = false;
    }
  }

  Future<void> _submit() async {
    final api = widget.api;
    if (api == null || _working || _prompt.text.trim().isEmpty) return;
    setState(() => _working = true);
    try {
      await api.request('POST', '/api/v1/ai/tasks', {
        'provider': _provider,
        'prompt': _prompt.text.trim(),
        'allow_edits': _allowEdits,
      });
      if (mounted && api == widget.api) {
        _prompt.clear();
        setState(() => _error = null);
        await _refresh();
      }
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _working = false);
    }
  }

  Future<void> _cancel(String id) async {
    final api = widget.api;
    if (api == null || _working) return;
    setState(() => _working = true);
    try {
      await api.request('POST', '/api/v1/ai/tasks/$id/cancel', {});
      await _refresh();
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _working = false);
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _timer?.cancel();
    _prompt.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final providers = (_status?['providers'] as List? ?? []).cast<JsonMap>();
    final selected = providers.where((p) => p['id'] == _provider).firstOrNull;
    final available = selected?['available'] == true && _error == null;
    final tasks = (_status?['tasks'] as List? ?? []).cast<JsonMap>();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text('AI任务', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 8),
        const Text('向本机 AI 助手提交 GPU Pocket 项目任务。电脑需保持运行。'),
        const SizedBox(height: 16),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(18),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                DropdownButtonFormField<String>(
                  initialValue: _provider,
                  decoration: const InputDecoration(labelText: '本机 AI'),
                  items: const [
                    DropdownMenuItem(value: 'codex', child: Text('Codex')),
                    DropdownMenuItem(
                      value: 'cursor',
                      child: Text('Cursor（桌面 Agent）'),
                    ),
                  ],
                  onChanged: _working
                      ? null
                      : (value) => setState(() => _provider = value!),
                ),
                const SizedBox(height: 12),
                Text(
                  widget.api == null
                      ? '请先在设置中连接第一台服务器'
                      : selected?['message']?.toString() ?? '正在检查本机 AI 连接',
                  style: TextStyle(
                    color: available ? Colors.greenAccent : Colors.amber,
                  ),
                ),
                if (_error != null) ...[
                  const SizedBox(height: 8),
                  Text(
                    _error!,
                    style: const TextStyle(color: Colors.orangeAccent),
                  ),
                ],
                const SizedBox(height: 16),
                const Text('工作目录', style: TextStyle(color: Colors.white60)),
                SelectableText(
                  _status?['workspace']?.toString() ??
                      r'D:\work\github\gpu-pocket',
                ),
                const SizedBox(height: 16),
                TextField(
                  controller: _prompt,
                  minLines: 3,
                  maxLines: 8,
                  maxLength: 2000,
                  onChanged: (_) => setState(() {}),
                  decoration: const InputDecoration(
                    labelText: '任务描述',
                    hintText: '提问、分析代码，或描述需要完成的项目修改',
                  ),
                ),
                SwitchListTile(
                  contentPadding: EdgeInsets.zero,
                  title: const Text('允许修改项目文件'),
                  subtitle: const Text('关闭时仅问答和分析；开启后允许本次任务修改代码。'),
                  value: _allowEdits,
                  onChanged: _working
                      ? null
                      : (value) => setState(() => _allowEdits = value),
                ),
                FilledButton.icon(
                  onPressed:
                      available && !_working && _prompt.text.trim().isNotEmpty
                      ? _submit
                      : null,
                  icon: const Icon(Icons.send),
                  label: Text(_working ? '正在提交' : '提交任务'),
                ),
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    TextButton.icon(
                      onPressed: _refresh,
                      icon: const Icon(Icons.refresh),
                      label: const Text('刷新 AI 状态'),
                    ),
                    TextButton(
                      onPressed: widget.onOpenSettings,
                      child: const Text('连接设置'),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 16),
        Text('最近任务', style: Theme.of(context).textTheme.titleMedium),
        if (tasks.isEmpty)
          const Padding(padding: EdgeInsets.all(16), child: Text('尚无任务')),
        for (final task in tasks)
          Card(
            child: ExpansionTile(
              title: Text(
                task['prompt']?.toString() ?? '',
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
              subtitle: Text(
                '${task['provider']} · ${_taskStatus(task['status'])}${task['allow_edits'] == true ? ' · 允许修改' : ' · 只读'}',
              ),
              childrenPadding: const EdgeInsets.all(16),
              expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                SelectableText(task['prompt']?.toString() ?? ''),
                const Divider(),
                SelectableText(
                  task['output']?.toString().isNotEmpty == true
                      ? task['output'].toString()
                      : '等待 AI 返回结果…',
                ),
                if ([
                  'queued',
                  'running',
                  'cancelling',
                ].contains(task['status']))
                  TextButton(
                    onPressed: _working || task['status'] == 'cancelling'
                        ? null
                        : () => _cancel(task['id'].toString()),
                    child: const Text('取消任务'),
                  ),
              ],
            ),
          ),
      ],
    );
  }

  String _taskStatus(dynamic status) => switch (status) {
    'queued' => '等待电脑领取',
    'running' => '正在运行',
    'cancelling' => '正在取消',
    'completed' => '已完成',
    'failed' => '失败',
    'cancelled' => '已取消',
    _ => '未知状态',
  };
}
