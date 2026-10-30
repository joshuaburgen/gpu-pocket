import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gpu_pocket/main.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
          const MethodChannel('plugins.it_nomads.com/flutter_secure_storage'),
          (call) async => null,
        );
  });
  testWidgets('first launch shows connection form and rejects public HTTP', (
    tester,
  ) async {
    await tester.pumpWidget(const GpuPocketApp());
    await tester.pumpAndSettle();
    expect(find.text('连接服务器'), findsOneWidget);
    expect(
      tester.widget<TextField>(find.byType(TextField).first).controller!.text,
      const String.fromEnvironment('GPU_POCKET_DEFAULT_URL'),
    );
    await tester.enterText(
      find.byType(TextField).at(0),
      'http://gpu.example.com',
    );
    await tester.enterText(
      find.byType(TextField).at(1),
      'test-only-token-with-at-least-32-characters',
    );
    await tester.tap(find.text('保存并连接'));
    await tester.pumpAndSettle();
    expect(find.textContaining('请输入 HTTPS 地址'), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
