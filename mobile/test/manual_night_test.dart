import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gpu_pocket/manual_night.dart';

void main() {
  testWidgets('independent manual night starts without an automation switch', (
    tester,
  ) async {
    var starts = 0;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ManualNightControl(
            active: false,
            selected: false,
            enabled: true,
            supported: true,
            memoryBlocked: false,
            onStart: () => starts++,
            onStop: () {},
          ),
        ),
      ),
    );
    await tester.tap(find.text('手动深夜（不避让）'));
    expect(starts, 1);
    expect(find.text('不受自动控制开关影响，不避让外部任务；到北京时间 08:00 自动停止。'), findsOneWidget);
  });

  testWidgets(
    'protected manual session can restart or exit without auto restart',
    (tester) async {
      var starts = 0, stops = 0;
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ManualNightControl(
              active: false,
              selected: true,
              enabled: true,
              supported: true,
              memoryBlocked: true,
              onStart: () => starts++,
              onStop: () => stops++,
            ),
          ),
        ),
      );
      expect(find.text('显存保护已停止矿工，恢复后需手动重新启动。'), findsOneWidget);
      await tester.tap(find.text('手动深夜（不避让）'));
      expect(starts, 1);
      await tester.tap(find.text('退出手动模式'));
      expect(stops, 1);
    },
  );

  testWidgets(
    'an active session remains stoppable if its current profile is unsupported',
    (tester) async {
      var stops = 0;
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ManualNightControl(
              active: true,
              selected: true,
              enabled: true,
              supported: false,
              memoryBlocked: false,
              onStart: () {},
              onStop: () => stops++,
            ),
          ),
        ),
      );
      await tester.tap(find.text('停止手动深夜'));
      expect(stops, 1);
    },
  );
}
