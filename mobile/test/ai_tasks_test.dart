import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gpu_pocket/ai_tasks.dart';
import 'package:gpu_pocket/api.dart';

class TaskApi extends GpuApi {
  TaskApi() : super('http://localhost', 'a' * 32);
  JsonMap? submitted;
  bool cancelled = false;

  @override
  Future<JsonMap> request(String method, String path, [JsonMap? body]) async {
    if (path == '/api/v1/ai/tasks') {
      submitted = body;
      return {
        'task': {'id': '123'},
      };
    }
    if (path.endsWith('/cancel')) {
      cancelled = true;
      return {'ok': true};
    }
    return {
      'workspace': r'D:\work\github\gpu-pocket',
      'providers': [
        {'id': 'codex', 'available': true, 'message': 'Codex ready'},
        {'id': 'cursor', 'available': false, 'message': '桌面桥接未启用'},
      ],
      'tasks': [
        if (submitted != null)
          {
            ...submitted!,
            'id': '123',
            'status': cancelled ? 'cancelled' : 'queued',
            'output': '',
          },
      ],
    };
  }
}

void main() {
  testWidgets('submits read-only Codex task and can cancel it', (tester) async {
    final api = TaskApi();
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: AiTasksPanel(api: api, onOpenSettings: () {}),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Codex ready'), findsOneWidget);
    await tester.enterText(find.byType(TextField), '解释空闲计时逻辑');
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('提交任务'));
    await tester.tap(find.text('提交任务'));
    await tester.pumpAndSettle();
    expect(api.submitted, {
      'provider': 'codex',
      'prompt': '解释空闲计时逻辑',
      'allow_edits': false,
    });
    await tester.ensureVisible(find.byType(ExpansionTile));
    await tester.tap(find.byType(ExpansionTile));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('取消任务'));
    await tester.tap(find.text('取消任务'));
    await tester.pumpAndSettle();
    expect(api.cancelled, isTrue);
    await tester.pumpWidget(const SizedBox.shrink());
    api.close();
  });

  testWidgets(
    'Cursor remains selectable and explains unavailable desktop bridge',
    (tester) async {
      final api = TaskApi();
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: AiTasksPanel(api: api, onOpenSettings: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byType(DropdownButtonFormField<String>));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Cursor（桌面 Agent）').last);
      await tester.pumpAndSettle();
      expect(find.text('桌面桥接未启用'), findsOneWidget);
      await tester.enterText(find.byType(TextField), 'test');
      final button = tester.widget<FilledButton>(
        find.widgetWithText(FilledButton, '提交任务'),
      );
      expect(button.onPressed, isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      api.close();
    },
  );
}
