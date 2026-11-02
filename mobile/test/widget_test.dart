import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gpu_pocket/main.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('active throttle mode uses purple while full mode stays green', () {
    expect(activeGpuModeColor('active', 'throttle'), Colors.purpleAccent);
    expect(
      activeGpuModeColor('active', 'manual_throttle'),
      Colors.purpleAccent,
    );
    expect(activeGpuModeColor('active', 'normal'), Colors.green);
    expect(activeGpuModeColor('inactive', 'throttle'), isNull);
  });

  setUp(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
          const MethodChannel('plugins.it_nomads.com/flutter_secure_storage'),
          (call) async => null,
        );
  });
  testWidgets('settings offers wallet presets in the worker field', (
    tester,
  ) async {
    await tester.pumpWidget(const GpuPocketApp());
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('设置'));
    await tester.pumpAndSettle();
    expect(find.text('统一挖矿配置'), findsOneWidget);
    expect(find.text('可选钱包 / 账户'), findsNothing);
    await tester.tap(find.byIcon(Icons.account_balance_wallet_outlined));
    await tester.pumpAndSettle();
    expect(find.text('Kryptex'), findsOneWidget);
    expect(find.text('NiceHash'), findsOneWidget);
    await tester.tap(find.text('Kryptex'));
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField).first).controller!.text,
      'krxXPVDK8N',
    );
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('AI task tab shows the GPU Pocket working directory', (
    tester,
  ) async {
    await tester.pumpWidget(const GpuPocketApp());
    await tester.pumpAndSettle();
    await tester.tap(find.text('AI任务').last);
    await tester.pumpAndSettle();
    expect(find.text('尚未连接本机 AI'), findsOneWidget);
    expect(find.text(r'D:\work\github\gpu-pocket'), findsOneWidget);
    expect(find.text('连接本机 AI 后可提交'), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
