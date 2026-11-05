import 'package:flutter/material.dart';

class ManualNightControl extends StatelessWidget {
  const ManualNightControl({
    super.key,
    required this.active,
    required this.selected,
    required this.enabled,
    required this.supported,
    required this.memoryBlocked,
    required this.onStart,
    required this.onStop,
  });

  final bool active, selected, enabled, supported, memoryBlocked;
  final VoidCallback onStart, onStop;

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      const Divider(height: 18),
      Wrap(
        spacing: 8,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          FilledButton.tonalIcon(
            onPressed: enabled && (active || supported)
                ? active
                      ? onStop
                      : onStart
                : null,
            icon: Icon(
              active ? Icons.stop_circle_outlined : Icons.nightlight_round,
            ),
            label: Text(active ? '停止手动深夜' : '手动深夜（不避让）'),
          ),
          if (selected && !active)
            TextButton(
              onPressed: enabled ? onStop : null,
              child: const Text('退出手动模式'),
            ),
        ],
      ),
      const SizedBox(height: 3),
      Text(
        selected && !active
            ? memoryBlocked
                  ? '显存保护已停止矿工，恢复后需手动重新启动。'
                  : '手动模式已停止，需要手动重新启动或退出。'
            : !supported && !active
            ? '需使用已暂存的 SRBMiner Pearl 配置及支持该模式的服务器。'
            : '不受自动控制开关影响，不避让外部任务；到北京时间 08:00 自动停止。',
        style: const TextStyle(color: Colors.white60, fontSize: 12),
      ),
    ],
  );
}
