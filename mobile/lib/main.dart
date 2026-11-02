import 'dart:async';

import 'package:firebase_core/firebase_core.dart';
import 'package:firebase_messaging/firebase_messaging.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'api.dart';

void main() => runApp(const GpuPocketApp());

String _kryptexWallet(int gpuId) => '';

String _niceHashWallet(int gpuId) => '';

class _GpuDraft {
  String? profileId;
  String? poolId;
  final wallet = TextEditingController();
  final powerLimit = TextEditingController();
  bool useGpuIntensity = false;
  double gpuIntensity = 16;
  bool dirty = false;

  void dispose() {
    wallet.dispose();
    powerLimit.dispose();
  }
}

class _GpuPolicyDraft {
  bool enabled = false;
  bool throttleEnabled = false;
  bool nightModeEnabled = false;
  bool externalTaskMonitor = false;
  final minFreeMemory = TextEditingController(text: '2048');
  bool dirty = false;

  void dispose() => minFreeMemory.dispose();
}

class _CpuDraft {
  String? profileId;
  String? poolId;
  final wallet = TextEditingController();
  double threadsPercent = 50;
  bool dirty = false;

  void dispose() => wallet.dispose();
}

class GpuPocketApp extends StatelessWidget {
  const GpuPocketApp({super.key});
  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'GPU Pocket',
    debugShowCheckedModeBanner: false,
    theme: ThemeData(
      colorScheme: ColorScheme.fromSeed(
        seedColor: const Color(0xFF6FE0B7),
        brightness: Brightness.dark,
      ),
      useMaterial3: true,
      scaffoldBackgroundColor: const Color(0xFF10171C),
      inputDecorationTheme: const InputDecorationTheme(
        border: OutlineInputBorder(),
      ),
    ),
    home: const Console(),
  );
}

class Console extends StatefulWidget {
  const Console({super.key});
  @override
  State<Console> createState() => _ConsoleState();
}

class _ConsoleState extends State<Console> with WidgetsBindingObserver {
  final _storage = const FlutterSecureStorage();
  final _url = TextEditingController();
  final _token = TextEditingController();
  final _url2 = TextEditingController();
  final _token2 = TextEditingController();
  final _sharedPoolId = TextEditingController();
  final _sharedWallet = TextEditingController();
  String? _sharedGpuProfileId;
  String? _sharedCpuProfileId;
  final Map<int, _GpuDraft> _gpuDrafts = {};
  final Map<int, _GpuDraft> _gpuDrafts2 = {};
  final Map<int, _GpuPolicyDraft> _policyDrafts = {};
  final Map<int, _GpuPolicyDraft> _policyDrafts2 = {};
  final _cpuDraft = _CpuDraft();
  final _cpuDraft2 = _CpuDraft();
  GpuApi? _api;
  GpuApi? _api2;
  JsonMap? _status, _status2, _catalog, _catalog2, _proxy;
  String? _error, _proxyError;
  String _pushServerStatus = '等待服务器状态';
  String _pushClientStatus = '尚未初始化';
  String _pushRegistrationStatus = '尚未提交设备令牌';
  String? _pushToken;
  StreamSubscription<String>? _pushTokenSubscription;
  StreamSubscription<RemoteMessage>? _foregroundMessageSubscription;
  DateTime? _lastSuccess;
  Timer? _timer;
  int _tab = 0, _generation = 0;
  bool _loading = false, _working = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _restore();
    _startTimer();
  }

  void _startTimer() {
    _timer?.cancel();
    _timer = Timer.periodic(const Duration(seconds: 15), (_) => _refresh());
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

  Future<void> _restore() async {
    try {
      final url = await _storage.read(key: 'server_url');
      final token = await _storage.read(key: 'server_token');
      final url2 = await _storage.read(key: 'server2_url');
      final token2 = await _storage.read(key: 'server2_token');
      final sharedPool = await _storage.read(key: 'miner_shared_pool');
      final sharedWallet = await _storage.read(key: 'miner_shared_wallet');
      final sharedGpuProfile = await _storage.read(
        key: 'miner_shared_gpu_profile',
      );
      final sharedCpuProfile = await _storage.read(
        key: 'miner_shared_cpu_profile',
      );
      if (!mounted) return;
      _url.text = url ?? const String.fromEnvironment('GPU_POCKET_DEFAULT_URL');
      _token.text = token ?? '';
      _url2.text = url2 ?? '';
      _token2.text = token2 ?? '';
      _sharedPoolId.text = sharedPool ?? '';
      _sharedWallet.text = sharedWallet ?? '';
      _sharedGpuProfileId = sharedGpuProfile;
      _sharedCpuProfileId = sharedCpuProfile;
      if (url != null && token != null) {
        await _connect(persist: false);
      }
      if (url2 != null && token2 != null) {
        await _connectSecond(persist: false);
      }
      if ((url == null || token == null) && (url2 == null || token2 == null)) {
        setState(() => _tab = 2);
      }
    } catch (e) {
      if (mounted) setState(() => _error = '读取安全存储失败：$e');
    }
  }

  Future<void> _connect({bool persist = true}) async {
    if (_working || _loading) return;
    try {
      final next = GpuApi(_url.text, _token.text.trim());
      if (_token.text.trim().length < 32) {
        next.close();
        throw ApiException('令牌至少需要 32 个字符。');
      }
      await _unregisterPush(_api);
      _api?.close();
      _api = next;
      _generation++;
      setState(() {
        _status = null;
        _catalog = null;
        _proxy = null;
        _lastSuccess = null;
        for (final draft in _gpuDrafts.values) {
          draft.dirty = false;
          draft.profileId = null;
          draft.poolId = null;
          draft.wallet.clear();
          draft.powerLimit.clear();
          draft.useGpuIntensity = false;
          draft.gpuIntensity = 16;
        }
        for (final draft in _policyDrafts.values) {
          draft.dirty = false;
        }
        _cpuDraft
          ..dirty = false
          ..profileId = null
          ..poolId = null
          ..threadsPercent = 50
          ..wallet.clear();
        _error = null;
      });
      if (persist) {
        await _storage.write(key: 'server_url', value: _url.text.trim());
        await _storage.write(key: 'server_token', value: _token.text.trim());
      }
      await _refresh();
      await _configurePush(next);
      if (mounted && _status != null) setState(() => _tab = 0);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _connectSecond({bool persist = true}) async {
    if (_working || _loading) return;
    try {
      final next = GpuApi(_url2.text, _token2.text.trim());
      if (_token2.text.trim().length < 32) {
        next.close();
        throw ApiException('第二台服务器的令牌至少需要 32 个字符。');
      }
      _api2?.close();
      _api2 = next;
      if (persist) {
        await _storage.write(key: 'server2_url', value: _url2.text.trim());
        await _storage.write(key: 'server2_token', value: _token2.text.trim());
      }
      await _refreshSecond();
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _refreshSecond() async {
    final api = _api2;
    if (api == null || !mounted) return;
    try {
      final status = await api.request('GET', '/api/v1/status');
      final catalog = _catalog2 == null
          ? await api.request('GET', '/api/v1/catalog')
          : _catalog2;
      if (mounted) {
        setState(() {
          _status2 = status;
          _catalog2 = catalog;
          _syncGpuDrafts();
          _syncPolicyDrafts();
          _syncSecondCpuDraft();
        });
      }
    } catch (e) {
      if (mounted) setState(() => _status2 = null);
    }
  }

  Future<void> _refresh() async {
    final api = _api;
    if (api == null) {
      await _refreshSecond();
      return;
    }
    if (_loading || _working || _hasTextInputFocus || !mounted) {
      return;
    }
    final generation = _generation;
    _loading = true;
    try {
      final results = await Future.wait([
        api.request('GET', '/api/v1/status'),
        if (_catalog == null) api.request('GET', '/api/v1/catalog'),
      ]);
      if (!mounted || generation != _generation || _hasTextInputFocus) return;
      final nextStatus = results.first;
      final newOutsideProcesses = _newOutsideProcesses(_status, nextStatus);
      setState(() {
        _status = nextStatus;
        if (results.length > 1) _catalog = results[1];
        _error = null;
        _lastSuccess = DateTime.now();
        _pushServerStatus = nextStatus['push_configured'] == true
            ? 'Firebase 凭据文件：存在（不代表通知已送达）'
            : 'Firebase 凭据文件：未配置';
        _syncGpuDrafts();
        _syncPolicyDrafts();
        _syncCpuDraft();
      });
      if (newOutsideProcesses.isNotEmpty &&
          (_pushToken == null || nextStatus['push_configured'] != true)) {
        final names = newOutsideProcesses
            .take(3)
            .map(
              (process) =>
                  'GPU${process['_gpu_id']}: ${process['name']} (PID ${process['pid']})',
            )
            .join('；');
        final remaining = newOutsideProcesses.length - 3;
        final suffix = remaining > 0 ? '；另有 $remaining 个' : '';
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('检测到 GPU 目录外进程：$names$suffix')));
      }
      if (_tab == 1) await _loadProxy(api);
    } catch (e) {
      if (mounted && generation == _generation && !_hasTextInputFocus) {
        setState(() {
          _error = e.toString();
          _pushServerStatus = '服务器状态读取失败：$e';
        });
      }
    } finally {
      _loading = false;
      await _refreshSecond();
      if (mounted) setState(() {});
    }
  }

  bool get _hasTextInputFocus =>
      FocusManager.instance.primaryFocus?.context?.widget is EditableText;

  List<JsonMap> _newOutsideProcesses(JsonMap? previous, JsonMap current) {
    if (previous == null) return [];
    final oldKeys = _outsideProcessMap(previous).keys.toSet();
    return _outsideProcessMap(current).entries
        .where((entry) => !oldKeys.contains(entry.key))
        .map((entry) => entry.value)
        .toList();
  }

  Map<String, JsonMap> _outsideProcessMap(JsonMap status) {
    final controls = status['gpu_controls'] as JsonMap? ?? {};
    final processes = <String, JsonMap>{};
    for (final entry in controls.entries) {
      final gpuId = entry.key;
      final control = entry.value;
      if (control is! Map) continue;
      for (final process in (control['processes'] as List?) ?? []) {
        if (process is! Map || process['within_gpu_root'] == true) continue;
        final name = process['name'] ?? 'unknown';
        final pid = process['pid'] ?? 'unknown';
        final path = process['executable'] ?? '';
        processes['$gpuId:$pid:$path:$name'] = {
          ...process.cast<String, dynamic>(),
          '_gpu_id': gpuId,
        };
      }
    }
    return processes;
  }

  Future<void> _configurePush(GpuApi api) async {
    await _pushTokenSubscription?.cancel();
    await _foregroundMessageSubscription?.cancel();
    _pushClientStatus = 'Firebase 初始化中';
    _pushRegistrationStatus = '等待获取 FCM Token';
    if (mounted) setState(() {});
    const apiKey = String.fromEnvironment('GPU_POCKET_FCM_API_KEY');
    const appId = String.fromEnvironment('GPU_POCKET_FCM_APP_ID');
    const senderId = String.fromEnvironment('GPU_POCKET_FCM_SENDER_ID');
    const projectId = String.fromEnvironment('GPU_POCKET_FCM_PROJECT_ID');
    if ([apiKey, appId, senderId, projectId].any((value) => value.isEmpty)) {
      if (mounted) {
        setState(() {
          _pushClientStatus = 'Firebase 初始化失败：缺少客户端配置参数';
          _pushRegistrationStatus = '未注册：Firebase 客户端未初始化';
        });
      }
      return;
    }

    try {
      if (Firebase.apps.isEmpty) {
        await Firebase.initializeApp(
          options: const FirebaseOptions(
            apiKey: apiKey,
            appId: appId,
            messagingSenderId: senderId,
            projectId: projectId,
          ),
        );
      } else {
        Firebase.app();
      }
      if (mounted) {
        setState(() => _pushClientStatus = 'Firebase 初始化成功；正在获取 FCM Token');
      }
    } catch (error) {
      if (mounted) {
        setState(() {
          _pushClientStatus = 'Firebase 初始化失败：$error';
          _pushRegistrationStatus = '未注册：Firebase 初始化失败';
        });
      }
      return;
    }

    final FirebaseMessaging messaging;
    final String token;
    try {
      messaging = FirebaseMessaging.instance;
      final permission = await messaging.requestPermission(
        alert: true,
        badge: true,
        sound: true,
      );
      if (permission.authorizationStatus == AuthorizationStatus.denied) {
        throw ApiException('通知权限已关闭，请在系统设置中允许 GPU Pocket 通知。');
      }
      // FCM HTTP v1 targets registration tokens; firebase_messaging 17's
      // register() stream exposes a Firebase Installation ID instead.
      // ignore: deprecated_member_use
      final result = await messaging.getToken();
      if (result == null) throw ApiException('Firebase 未返回设备推送令牌。');
      token = result;
      if (mounted) {
        setState(() => _pushClientStatus = 'Firebase 初始化成功；FCM Token 获取成功');
      }
    } catch (error) {
      if (mounted) {
        setState(() {
          _pushClientStatus = 'Firebase 初始化成功；FCM Token 获取失败：$error';
          _pushRegistrationStatus = '未注册：没有可提交的 FCM Token';
        });
      }
      return;
    }

    try {
      await api.request('PUT', '/api/v1/push/token', {'token': token});
      _pushToken = token;
      if (mounted) {
        setState(() => _pushRegistrationStatus = 'FCM Token 已成功提交并登记到服务器');
      }
      // ignore: deprecated_member_use
      _pushTokenSubscription = messaging.onTokenRefresh.listen(
        (token) async {
          if (!identical(_api, api)) return;
          if (mounted) {
            setState(() => _pushRegistrationStatus = 'FCM Token 已更新；正在登记到服务器');
          }
          try {
            final previousToken = _pushToken;
            await api.request('PUT', '/api/v1/push/token', {'token': token});
            _pushToken = token;
            if (previousToken != null && previousToken != token) {
              await api.request('DELETE', '/api/v1/push/token', {
                'token': previousToken,
              });
            }
            if (mounted) {
              setState(
                () => _pushRegistrationStatus = '更新后的 FCM Token 已成功登记到服务器',
              );
            }
          } catch (error) {
            if (mounted) {
              setState(
                () => _pushRegistrationStatus = 'FCM Token 更新登记失败：$error',
              );
            }
          }
        },
        onError: (Object error) {
          if (mounted) {
            setState(() => _pushClientStatus = 'FCM Token 刷新监听失败：$error');
          }
        },
      );
      _foregroundMessageSubscription = FirebaseMessaging.onMessage.listen((
        message,
      ) {
        final notification = message.notification;
        if (mounted && notification != null) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text(
                '${notification.title ?? 'GPU Pocket'}：${notification.body ?? ''}',
              ),
            ),
          );
        }
      });
    } catch (error) {
      if (mounted) {
        setState(() => _pushRegistrationStatus = 'FCM Token 登记失败：$error');
      }
    }
  }

  Future<void> _unregisterPush(GpuApi? api) async {
    await _pushTokenSubscription?.cancel();
    _pushTokenSubscription = null;
    await _foregroundMessageSubscription?.cancel();
    _foregroundMessageSubscription = null;
    final token = _pushToken;
    _pushToken = null;
    if (api == null || token == null) return;
    try {
      await api.request('DELETE', '/api/v1/push/token', {'token': token});
      if (mounted) {
        setState(() => _pushRegistrationStatus = '此设备的 FCM Token 已从服务器注销');
      }
    } catch (error) {
      if (mounted) {
        setState(() => _pushRegistrationStatus = '注销服务器 FCM Token 失败：$error');
      }
    }
  }

  void _syncGpuDrafts() {
    _syncGpuDraftsFrom(_status, _gpuDrafts);
    _syncGpuDraftsFrom(_status2, _gpuDrafts2);
  }

  void _syncGpuDraftsFrom(JsonMap? status, Map<int, _GpuDraft> drafts) {
    final controls = (status?['gpu_controls'] as JsonMap?) ?? {};
    for (final entry in controls.entries) {
      final gpuId = int.tryParse(entry.key);
      if (gpuId == null || entry.value is! Map) continue;
      final draft = drafts.putIfAbsent(gpuId, _GpuDraft.new);
      if (draft.dirty) continue;
      final desired = (entry.value as JsonMap)['desired'] as JsonMap?;
      draft.profileId = desired?['profile_id'] as String?;
      draft.poolId = desired?['pool_id'] as String?;
      _setTextIfChanged(draft.wallet, desired?['wallet'] as String? ?? '');
      final power = desired?['power_limit_w'];
      _setTextIfChanged(
        draft.powerLimit,
        power is num ? power.toInt().toString() : '',
      );
    }
  }

  void _syncPolicyDrafts() {
    _syncPolicyDraftsFrom(_status, _policyDrafts);
    _syncPolicyDraftsFrom(_status2, _policyDrafts2);
  }

  void _syncPolicyDraftsFrom(
    JsonMap? status,
    Map<int, _GpuPolicyDraft> drafts,
  ) {
    final policies = (status?['policies'] as JsonMap?) ?? {};
    for (final entry in policies.entries) {
      final gpuId = int.tryParse(entry.key);
      if (gpuId == null || entry.value is! Map) continue;
      final draft = drafts.putIfAbsent(gpuId, _GpuPolicyDraft.new);
      if (draft.dirty) continue;
      final policy = (entry.value as JsonMap)['policy'] as JsonMap?;
      if (policy == null) continue;
      draft.enabled = policy['enabled'] == true;
      draft.throttleEnabled = policy['throttle_enabled'] == true;
      draft.nightModeEnabled = policy['night_mode_enabled'] == true;
      draft.externalTaskMonitor = policy['external_task_monitor'] == true;
      final memory = policy['min_free_memory_mib'];
      _setTextIfChanged(
        draft.minFreeMemory,
        memory is num ? memory.toInt().toString() : '2048',
      );
    }
  }

  void _syncSecondCpuDraft() {
    if (_cpuDraft2.dirty) return;
    final desired =
        (_status2?['cpu_mining'] as JsonMap?)?['desired'] as JsonMap?;
    _cpuDraft2
      ..profileId = desired?['profile_id'] as String?
      ..poolId = desired?['pool_id'] as String?
      ..threadsPercent =
          (desired?['threads_percent'] as num?)?.toDouble() ?? 50;
    _setTextIfChanged(_cpuDraft2.wallet, desired?['wallet'] as String? ?? '');
  }

  void _syncCpuDraft() {
    if (_cpuDraft.dirty) return;
    final desired =
        (_status?['cpu_mining'] as JsonMap?)?['desired'] as JsonMap?;
    _cpuDraft
      ..profileId = desired?['profile_id'] as String?
      ..poolId = desired?['pool_id'] as String?
      ..threadsPercent =
          (desired?['threads_percent'] as num?)?.toDouble() ?? 50;
    _setTextIfChanged(_cpuDraft.wallet, desired?['wallet'] as String? ?? '');
  }

  void _setTextIfChanged(TextEditingController controller, String value) {
    if (controller.text == value) return;
    controller.value = TextEditingValue(
      text: value,
      selection: TextSelection.collapsed(offset: value.length),
    );
  }

  Future<void> _loadProxy(GpuApi api) async {
    try {
      final proxy = await api.request('GET', '/api/v1/proxy');
      if (mounted) {
        setState(() {
          _proxy = proxy;
          _proxyError = null;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _proxy = null;
          _proxyError = e.toString();
        });
      }
    }
  }

  Future<void> _mutateSecond(Future<void> Function(GpuApi) operation) async {
    if (_api2 == null || _working || _loading) return;
    String? operationError;
    setState(() => _working = true);
    try {
      await operation(_api2!);
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(const SnackBar(content: Text('操作完成')));
      }
    } catch (e) {
      operationError = e.toString();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(operationError),
            duration: const Duration(seconds: 12),
          ),
        );
      }
    } finally {
      if (mounted) {
        setState(() => _working = false);
        await _refreshSecond();
        if (mounted && operationError != null)
          setState(() => _error = operationError);
      }
    }
  }

  Future<void> _mutate(Future<void> Function(GpuApi) operation) async {
    if (_api == null || _working || _loading) return;
    String? operationError;
    setState(() => _working = true);
    try {
      await operation(_api!);
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(const SnackBar(content: Text('操作完成')));
      }
    } catch (e) {
      operationError = e.toString();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(operationError),
            duration: const Duration(seconds: 12),
          ),
        );
      }
    } finally {
      if (mounted) {
        setState(() => _working = false);
        await _refresh();
        if (mounted && operationError != null) {
          setState(() => _error = operationError);
        }
      }
    }
  }

  Future<bool> _confirm(String title, String body) async =>
      await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: Text(title),
          content: Text(body),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('确认'),
            ),
          ],
        ),
      ) ??
      false;

  int _revisionFor(int gpuId, {bool second = false}) {
    final status = second ? _status2 : _status;
    return (((status?['gpu_controls'] as JsonMap?)?[gpuId.toString()]
                    as JsonMap?)?['desired']
                as JsonMap?)?['revision']
            as int? ??
        0;
  }

  bool get _enabled =>
      _api != null && _status?['control_enabled'] == true && !_working;
  List<JsonMap> get _profiles =>
      ((_catalog?['profiles'] as List?) ?? []).cast<JsonMap>();
  List<JsonMap> get _cpuProfiles =>
      ((_catalog?['cpu_profiles'] as List?) ?? []).cast<JsonMap>();

  int get _cpuRevision =>
      (((_status?['cpu_mining'] as JsonMap?)?['desired'])
              as JsonMap?)?['revision']
          as int? ??
      0;

  Future<void> _stageCpu() async {
    if (!_enabled ||
        _cpuDraft.profileId == null ||
        _cpuDraft.poolId == null ||
        _cpuDraft.wallet.text.trim().isEmpty) {
      return;
    }
    await _mutate((api) async {
      await api.request('PUT', '/api/v1/cpu/selection', {
        'profile_id': _cpuDraft.profileId,
        'pool_id': _cpuDraft.poolId,
        'wallet': _cpuDraft.wallet.text.trim(),
        'threads_percent': _cpuDraft.threadsPercent.round(),
        'revision': _cpuRevision,
      });
      _cpuDraft.dirty = false;
    });
  }

  bool get _enabled2 =>
      _api2 != null && _status2?['control_enabled'] == true && !_working;

  int get _cpuRevision2 =>
      (((_status2?['cpu_mining'] as JsonMap?)?['desired'])
              as JsonMap?)?['revision']
          as int? ??
      0;

  Future<void> _stageCpu2() async {
    if (!_enabled2 ||
        _cpuDraft2.profileId == null ||
        _cpuDraft2.poolId == null ||
        _cpuDraft2.wallet.text.trim().isEmpty)
      return;
    try {
      await _api2!.request('PUT', '/api/v1/cpu/selection', {
        'profile_id': _cpuDraft2.profileId,
        'pool_id': _cpuDraft2.poolId,
        'wallet': _cpuDraft2.wallet.text.trim(),
        'threads_percent': _cpuDraft2.threadsPercent.round(),
        'revision': _cpuRevision2,
      });
      _cpuDraft2.dirty = false;
      await _refreshSecond();
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _cpuMiner2(String operation) async {
    if (!_enabled2) return;
    try {
      await _api2!.request('POST', '/api/v1/cpu/miner', {
        'operation': operation,
        'revision': _cpuRevision2,
      });
      await _refreshSecond();
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<void> _cpuMiner(String operation) async {
    if (!_enabled) return;
    final selected =
        (_status?['cpu_mining'] as JsonMap?)?['desired'] as JsonMap?;
    final details = operation == 'stop'
        ? '停止 GPU Pocket 管理的 CPU 矿工。'
        : '应用 CPU 配置：\n算法：${selected?['profile_id']}\n矿池：${selected?['pool_id']}\n线程比例：${selected?['threads_percent'] ?? 50}%\n此比例是 SRBMiner 线程提示，不是严格的系统级 CPU 限额。';
    if (await _confirm(
      operation == 'stop' ? '停止 CPU 矿工' : '应用并启动 CPU 矿工',
      details,
    )) {
      await _mutate((api) async {
        await api.request('POST', '/api/v1/cpu/miner', {
          'operation': operation,
          'revision': _cpuRevision,
        });
      });
    }
  }

  Future<void> _miner(int gpuId, String operation) async {
    if (!_enabled) return;
    final control =
        (_status?['gpu_controls'] as JsonMap?)?[gpuId.toString()] as JsonMap?;
    final selected = control?['desired'] as JsonMap?;
    final revision = _revisionFor(gpuId);
    final draft = _gpuDrafts.putIfAbsent(gpuId, _GpuDraft.new);
    final profile = _profileFor(_catalog, selected?['profile_id'] as String?);
    final intensity = draft.useGpuIntensity && profile?['miner'] == 'srb-pearl'
        ? draft.gpuIntensity.round()
        : null;
    final details = operation == 'stop'
        ? '停止 GPU Pocket 管理的 GPU$gpuId 矿工。'
        : 'GPU$gpuId 将使用以下已暂存配置：\n算法：${selected?['profile_id']}\n矿池：${selected?['pool_id']}\n功耗上限：${selected?['power_limit_w'] ?? '系统默认'} W\nSRBMiner 强度：${intensity ?? '默认'}\n应用时可能中断当前计算。';
    if (await _confirm(
      operation == 'stop' ? '停止 GPU$gpuId' : '应用并重启 GPU$gpuId',
      details,
    )) {
      await _mutate((api) async {
        await api.request('POST', '/api/v1/gpus/$gpuId/miner', {
          'operation': operation,
          'revision': revision,
          if (operation != 'stop' && intensity != null)
            'gpu_intensity': intensity,
        });
      });
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _timer?.cancel();
    _api?.close();
    _api2?.close();
    _url.dispose();
    _token.dispose();
    _url2.dispose();
    _token2.dispose();
    _sharedPoolId.dispose();
    _sharedWallet.dispose();
    for (final draft in [..._gpuDrafts.values, ..._gpuDrafts2.values]) {
      draft.dispose();
    }
    for (final draft in [..._policyDrafts.values, ..._policyDrafts2.values]) {
      draft.dispose();
    }
    _cpuDraft.dispose();
    _cpuDraft2.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(
      title: const Text('GPU Pocket'),
      actions: [
        if (_loading || _working)
          const Padding(
            padding: EdgeInsets.all(18),
            child: SizedBox(
              width: 20,
              height: 20,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
          ),
        IconButton(
          onPressed: _refresh,
          icon: const Icon(Icons.refresh),
          tooltip: '刷新',
        ),
      ],
    ),
    body: RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.all(20),
        physics: const AlwaysScrollableScrollPhysics(),
        children: [
          if (_status?['demo'] == true)
            _notice('演示模式：模拟数据，操作不会影响真实服务器。', Colors.amber),
          if (_error != null) _notice(_error!, Colors.orangeAccent),
          ...switch (_tab) {
            0 => _overview(),
            1 => _controls(),
            _ => _settings(),
          },
        ],
      ),
    ),
    bottomNavigationBar: NavigationBar(
      selectedIndex: _tab,
      onDestinationSelected: (i) {
        setState(() => _tab = i);
        if (i == 1) _refresh();
      },
      destinations: const [
        NavigationDestination(icon: Icon(Icons.memory), label: '监控'),
        NavigationDestination(icon: Icon(Icons.tune), label: '控制'),
        NavigationDestination(icon: Icon(Icons.settings), label: '设置'),
      ],
    ),
  );

  Widget _notice(String text, Color color) => Container(
    margin: const EdgeInsets.only(bottom: 16),
    padding: const EdgeInsets.all(14),
    decoration: BoxDecoration(
      color: color.withAlpha(25),
      borderRadius: BorderRadius.circular(12),
    ),
    child: Text(text, style: TextStyle(color: color)),
  );

  Widget _heading(String title, String subtitle) => Padding(
    padding: const EdgeInsets.only(bottom: 20),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(title, style: Theme.of(context).textTheme.headlineMedium),
        const SizedBox(height: 6),
        Text(subtitle, style: const TextStyle(color: Colors.white60)),
      ],
    ),
  );

  List<Widget> _overview() {
    final last = _lastSuccess;
    return [
      _heading(
        '服务器监控',
        last == null
            ? '连接服务器后查看实时状态'
            : '每 15 秒刷新 · 最近成功 ${last.hour.toString().padLeft(2, '0')}:${last.minute.toString().padLeft(2, '0')}:${last.second.toString().padLeft(2, '0')}',
      ),
      if (_status != null)
        _serverOverviewCard('2-titan', _status!, showErrors: true),
      if (_status == null && _api != null)
        _notice('2-titan 当前无法读取状态。', Colors.orangeAccent),
      if (_status2 != null) _serverOverviewCard('zww', _status2!),
      if (_status2 == null && _api2 != null)
        _notice('zww 当前无法读取状态。', Colors.orangeAccent),
    ];
  }

  Widget _serverOverviewCard(
    String label,
    JsonMap status, {
    bool showErrors = false,
  }) {
    final gpus = (status['gpus'] as List?)?.cast<JsonMap>() ?? [];
    final controls = status['gpu_controls'] as JsonMap? ?? {};
    final cpu = status['cpu_mining'] as JsonMap? ?? {};
    final cpuService = cpu['service'] ?? 'unknown';
    final cpuActive = cpu['active'] is Map ? cpu['active'] as JsonMap : null;
    return Card(
      margin: const EdgeInsets.only(bottom: 16),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(label, style: Theme.of(context).textTheme.headlineSmall),
            const SizedBox(height: 12),
            if (showErrors)
              for (final error in (status['errors'] as List?) ?? [])
                _notice(error.toString(), Colors.orangeAccent),
            _cpuOverviewRow(cpu),
            for (final gpu in gpus) _gpuCard(gpu, controls),
            if (gpus.isEmpty) const Text('暂无 GPU 数据。请检查服务器 nvidia-smi。'),
          ],
        ),
      ),
    );
  }

  Widget _serverStatusCard(JsonMap status) {
    final gpuList = (status['gpus'] as List?)?.cast<JsonMap>() ?? [];
    final cpu = status['cpu_mining'] as JsonMap? ?? {};
    return Card(
      margin: const EdgeInsets.only(bottom: 16),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              status['host'] as String? ?? '第二台服务器',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            for (final gpu in gpuList)
              Text(
                'GPU ${gpu['index']} · ${gpu['utilization'] ?? '—'}% · ${gpu['temperature_c'] ?? '—'} °C · ${gpu['name']}',
              ),
            Text(
              'CPU Miner · ${cpu['service'] ?? '未知'} · ${((cpu['active'] as JsonMap?)?['threads_percent'] ?? '—')}% threads',
            ),
          ],
        ),
      ),
    );
  }

  String _number(dynamic value, [String unit = '']) => value is num
      ? '${value.toStringAsFixed(value == value.roundToDouble() ? 0 : 1)}$unit'
      : '—';

  String _idleDuration(dynamic value) {
    if (value is! num) return '未空闲';
    final seconds = value.round().clamp(0, 1 << 31);
    final hours = seconds ~/ 3600;
    final minutes = (seconds % 3600) ~/ 60;
    final remainder = seconds % 60;
    if (hours > 0) {
      return '$hours:${minutes.toString().padLeft(2, '0')}:${remainder.toString().padLeft(2, '0')}';
    }
    return '${minutes.toString().padLeft(2, '0')}:${remainder.toString().padLeft(2, '0')}';
  }

  Color _cpuOverviewColor(JsonMap cpu) {
    final service = cpu['service']?.toString();
    if (service == 'active') return Colors.green;
    if (service == 'failed' || service == 'unknown') return Colors.redAccent;
    return Colors.grey;
  }

  Widget _cpuOverviewRow(JsonMap cpu) {
    final service = cpu['service']?.toString() ?? 'unknown';
    final usage = cpu['usage_percent'];
    final active = cpu['active'] as JsonMap?;
    final selected =
        active?['threads_percent'] ?? cpu['desired']?['threads_percent'];
    final color = _cpuOverviewColor(cpu);
    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
      decoration: BoxDecoration(
        color: color.withAlpha(35),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: color.withAlpha(150)),
      ),
      child: Row(
        children: [
          Icon(Icons.memory, size: 16, color: color),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              'CPU $service · 使用率 ${usage is num ? '${usage.round()}%' : '采样中'} · 可控 ${selected ?? '—'}%',
              style: TextStyle(
                color: color == Colors.grey ? Colors.white70 : color,
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _gpuCard(JsonMap gpu, JsonMap controls) {
    final utilization = gpu['utilization'] as num?;
    final gpuId = gpu['index'] as num?;
    final control = gpuId == null
        ? null
        : controls[gpuId.toInt().toString()] as JsonMap?;
    final service = control?['service']?.toString();
    final external = control?['external_pids'] as List?;
    final processes = control?['processes'] as List?;
    final hasExternal =
        (external?.isNotEmpty ?? false) || (processes?.isNotEmpty ?? false);
    final color = service == 'active'
        ? Colors.green
        : hasExternal
        ? Colors.amber
        : (service == 'inactive' || service == null)
        ? Colors.grey
        : Colors.redAccent;
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      color: color.withAlpha(18),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Container(
                  width: 8,
                  height: 8,
                  decoration: BoxDecoration(
                    color: color,
                    shape: BoxShape.circle,
                  ),
                ),
                const SizedBox(width: 7),
                Expanded(
                  child: Text(
                    'GPU ${gpu['index']} · ${gpu['name'] ?? '未知设备'}',
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                Text(
                  _number(utilization, '%'),
                  style: Theme.of(
                    context,
                  ).textTheme.titleMedium?.copyWith(color: color),
                ),
              ],
            ),
            const SizedBox(height: 5),
            LinearProgressIndicator(
              minHeight: 3,
              value: utilization == null
                  ? 0
                  : (utilization / 100).clamp(0.0, 1.0),
              color: color,
              backgroundColor: Colors.white10,
            ),
            const SizedBox(height: 5),
            Text(
              '温度 ${_number(gpu['temperature_c'], ' °C')} · 功耗 ${_number(gpu['power_w'], ' W')} · 风扇 ${_number(gpu['fan_percent'], '%')} · 显存 ${_number(gpu['memory_used_mib'])}/${_number(gpu['memory_total_mib'], ' MiB')}',
              style: const TextStyle(color: Colors.white70, fontSize: 12),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            const SizedBox(height: 3),
            Text(
              '当前空闲 ${_idleDuration(control?['idle_seconds'])}',
              style: const TextStyle(color: Colors.white60, fontSize: 12),
            ),
          ],
        ),
      ),
    );
  }

  Widget _cpuCollapsedSummary(
    JsonMap? status,
    bool enabled,
    ValueChanged<int> onPercent,
  ) {
    final cpu = status?['cpu_mining'] as JsonMap?;
    final usage = cpu?['usage_percent'];
    final active = cpu?['active'] as JsonMap?;
    final desired = cpu?['desired'] as JsonMap?;
    final selected = active?['threads_percent'] ?? desired?['threads_percent'];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          '当前占用 ${usage is num ? '$usage%' : '采样中'} · 已设 ${selected ?? '—'}%',
        ),
        Wrap(
          spacing: 2,
          children: [25, 50, 75, 100]
              .map(
                (percent) => TextButton(
                  onPressed: enabled ? () => onPercent(percent) : null,
                  style: TextButton.styleFrom(
                    minimumSize: const Size(44, 32),
                    padding: const EdgeInsets.symmetric(horizontal: 4),
                  ),
                  child: Text('$percent%'),
                ),
              )
              .toList(),
        ),
      ],
    );
  }

  Color _gpuStatusColor(JsonMap? control, JsonMap? metrics) {
    final service = control?['service']?.toString();
    if (service == 'active') return Colors.greenAccent;
    final external = control?['external_pids'] as List?;
    final processes = control?['processes'] as List?;
    if ((external?.isNotEmpty ?? false) || (processes?.isNotEmpty ?? false))
      return Colors.amber;
    return service == 'inactive' ? Colors.grey : Colors.blueGrey;
  }

  Widget _gpuStatusDot(JsonMap? control, JsonMap? metrics) => Container(
    width: 12,
    height: 12,
    decoration: BoxDecoration(
      color: _gpuStatusColor(control, metrics),
      shape: BoxShape.circle,
    ),
  );

  Future<void> _setCpuThreads1(int percent) async {
    final desired =
        (_status?['cpu_mining'] as JsonMap?)?['desired'] as JsonMap?;
    if (desired?['profile_id'] == null ||
        desired?['pool_id'] == null ||
        desired?['wallet'] == null)
      return;
    setState(() {
      _cpuDraft.profileId = desired!['profile_id'] as String;
      _cpuDraft.poolId = desired['pool_id'] as String;
      _cpuDraft.wallet.text = desired['wallet'] as String;
      _cpuDraft.threadsPercent = percent.toDouble();
      _cpuDraft.dirty = true;
    });
    await _stageCpu();
  }

  Future<void> _setCpuThreads2(int percent) async {
    final desired =
        (_status2?['cpu_mining'] as JsonMap?)?['desired'] as JsonMap?;
    if (desired?['profile_id'] == null ||
        desired?['pool_id'] == null ||
        desired?['wallet'] == null)
      return;
    setState(() {
      _cpuDraft2.profileId = desired!['profile_id'] as String;
      _cpuDraft2.poolId = desired['pool_id'] as String;
      _cpuDraft2.wallet.text = desired['wallet'] as String;
      _cpuDraft2.threadsPercent = percent.toDouble();
      _cpuDraft2.dirty = true;
    });
    await _stageCpu2();
  }

  List<Widget> _controls() {
    return [
      _heading('设备控制', '展开服务器，再展开需要操作的设备。'),
      if (_api != null)
        ExpansionTile(
          initiallyExpanded: true,
          leading: const Icon(Icons.memory),
          title: const Text('2-titan'),
          subtitle: Text(
            _status?['control_enabled'] == true ? '控制已启用' : '只读模式',
          ),
          children: [
            ExpansionTile(
              title: const Text('CPU'),
              subtitle: _cpuCollapsedSummary(
                _status,
                _enabled,
                (percent) => _setCpuThreads1(percent),
              ),
              children: [_cpuControlCard()],
            ),
            for (final gpuId in [0, 1]) _firstGpuControlSection(gpuId),
          ],
        ),
      if (_api2 != null)
        ExpansionTile(
          initiallyExpanded: false,
          leading: const Icon(Icons.memory_outlined),
          title: const Text('zww'),
          subtitle: Text(
            _status2?['control_enabled'] == true ? '控制已启用' : '只读模式',
          ),
          children: [
            ExpansionTile(
              title: const Text('CPU'),
              subtitle: _cpuCollapsedSummary(
                _status2,
                _enabled2,
                (percent) => _setCpuThreads2(percent),
              ),
              children: [_secondCpuControlCard()],
            ),
            for (final gpuId in [0, 1]) _secondGpuControlSection(gpuId),
          ],
        ),
      ExpansionTile(
        leading: const Icon(Icons.notifications_outlined),
        title: const Text('高级：通知与代理'),
        children: [_pushStatusPanel(), ..._proxyControls()],
      ),
    ];
  }

  Widget _firstGpuControlSection(int gpuId) {
    final controls = (_status?['gpu_controls'] as JsonMap?) ?? {};
    final metrics = ((_status?['gpus'] as List?) ?? []).cast<JsonMap>();
    return ExpansionTile(
      leading: _gpuStatusDot(
        controls['$gpuId'] as JsonMap?,
        metrics.where((gpu) => gpu['index'] == gpuId).firstOrNull,
      ),
      title: Text('GPU$gpuId'),
      children: [
        _gpuControlCard(
          gpuId,
          controls['$gpuId'] as JsonMap?,
          metrics.where((gpu) => gpu['index'] == gpuId).firstOrNull,
        ),
      ],
    );
  }

  Future<void> _secondGpuMiner(int gpuId, String operation) async {
    if (!_enabled2) return;
    final control =
        (_status2?['gpu_controls'] as JsonMap?)?[gpuId.toString()] as JsonMap?;
    final desired = control?['desired'] as JsonMap?;
    final revision = desired?['revision'] as int? ?? 0;
    final draft = _gpuDrafts2.putIfAbsent(gpuId, _GpuDraft.new);
    final profile = _profileFor(_catalog2, desired?['profile_id'] as String?);
    final intensity = draft.useGpuIntensity && profile?['miner'] == 'srb-pearl'
        ? draft.gpuIntensity.round()
        : null;
    if (!await _confirm(
      operation == 'stop' ? '停止 zww GPU$gpuId' : '重启 zww GPU$gpuId',
      operation == 'stop'
          ? '停止 zww GPU$gpuId Miner。'
          : '应用 GPU$gpuId 的已暂存配置并重启 Miner。\nSRBMiner 强度：${intensity ?? '默认'}',
    ))
      return;
    try {
      await _api2!.request('POST', '/api/v1/gpus/$gpuId/miner', {
        'operation': operation,
        'revision': revision,
        if (operation != 'stop' && intensity != null)
          'gpu_intensity': intensity,
      });
      await _refreshSecond();
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Widget _secondGpuControlSection(int gpuId) {
    final control =
        (_status2?['gpu_controls'] as JsonMap?)?[gpuId.toString()] as JsonMap?;
    final gpuMetrics = ((_status2?['gpus'] as List?) ?? [])
        .cast<JsonMap>()
        .where((gpu) => gpu['index'] == gpuId)
        .firstOrNull;
    return ExpansionTile(
      leading: _gpuStatusDot(control, gpuMetrics),
      title: Text('GPU$gpuId'),
      subtitle: Text(gpuMetrics?['name'] as String? ?? '设备状态未知'),
      children: [_gpuControlCard(gpuId, control, gpuMetrics, second: true)],
    );
  }

  JsonMap? _profileFor(JsonMap? catalog, String? profileId) {
    if (profileId == null) return null;
    return ((catalog?['profiles'] as List?) ?? [])
        .whereType<JsonMap>()
        .where((profile) => profile['id'] == profileId)
        .firstOrNull;
  }

  Widget _secondCpuControlCard() {
    final draft = _cpuDraft2;
    final cpu = _status2?['cpu_mining'] as JsonMap?;
    final service = cpu?['service'] ?? 'unknown';
    return Card(
      margin: const EdgeInsets.only(top: 18),
      child: Padding(
        padding: const EdgeInsets.all(18),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              '${_status2?['host'] ?? '第二台服务器'} CPU · $service',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const SizedBox(height: 14),
            const Text(
              '算法、矿池和钱包已统一在“设置”中管理。',
              style: TextStyle(color: Colors.white70),
            ),
            Text('当前：${draft.profileId ?? '未配置'} · ${draft.poolId ?? '未配置'}'),
            const SizedBox(height: 14),
            Text('CPU 线程比例：${draft.threadsPercent.round()}%'),
            Slider(
              value: draft.threadsPercent.clamp(1, 100),
              min: 1,
              max: 100,
              divisions: 99,
              label: '${draft.threadsPercent.round()}%',
              onChanged: _enabled2
                  ? (value) => setState(() {
                      draft.threadsPercent = value;
                      draft.dirty = true;
                    })
                  : null,
            ),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                FilledButton(
                  onPressed: draft.dirty && _enabled2 ? _stageCpu2 : null,
                  child: const Text('暂存 CPU 配置'),
                ),
                FilledButton(
                  onPressed: _enabled2 ? () => _cpuMiner2('start') : null,
                  child: const Text('启动 CPU'),
                ),
                FilledButton(
                  onPressed: _enabled2 ? () => _cpuMiner2('restart') : null,
                  child: const Text('重启 CPU'),
                ),
                OutlinedButton(
                  onPressed: _enabled2 ? () => _cpuMiner2('stop') : null,
                  child: const Text('停止 CPU'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _cpuControlCard() {
    final draft = _cpuDraft;
    final cpuProfiles = _cpuProfiles;
    final cpu = _status?['cpu_mining'] as JsonMap?;
    final service = cpu?['service'] ?? 'unknown';
    final desired = cpu?['desired'] as JsonMap?;
    final canStage =
        _enabled &&
        draft.dirty &&
        draft.profileId != null &&
        draft.poolId != null &&
        draft.wallet.text.trim().isNotEmpty;
    return Card(
      color: const Color(0xFF20292E),
      margin: const EdgeInsets.only(top: 18),
      child: Padding(
        padding: const EdgeInsets.all(18),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'CPU 挖矿 · $service',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const SizedBox(height: 8),
            const Text(
              '独立于 GPU0 / GPU1。默认线程比例 50%，可调至 1–100%；这是 SRBMiner 的线程提示，不是严格 CPU 硬限额。',
              style: TextStyle(color: Colors.white60),
            ),
            if (cpuProfiles.isEmpty)
              _notice('服务器尚未配置 CPU 算法与矿池白名单。请到“设置”统一配置。', Colors.amber)
            else ...[
              const SizedBox(height: 14),
              const Text(
                '算法、矿池和钱包已统一在“设置”中管理。',
                style: TextStyle(color: Colors.white70),
              ),
              const SizedBox(height: 10),
              Text('CPU 线程比例：${draft.threadsPercent.round()}%'),
              Slider(
                min: 1,
                max: 100,
                divisions: 99,
                value: draft.threadsPercent.clamp(1, 100),
                label: '${draft.threadsPercent.round()}%',
                onChanged: _enabled
                    ? (value) => setState(() {
                        draft
                          ..threadsPercent = value
                          ..dirty = true;
                      })
                    : null,
              ),
              FilledButton.tonal(
                onPressed: canStage ? _stageCpu : null,
                child: Text(
                  '暂存 CPU 配置${desired == null ? '' : ' · r${desired['revision']}'}',
                ),
              ),
            ],
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              children: [
                FilledButton(
                  onPressed: _enabled && desired != null && service != 'active'
                      ? () => _cpuMiner('start')
                      : null,
                  child: const Text('启动 CPU'),
                ),
                OutlinedButton(
                  onPressed: _enabled && desired != null && service == 'active'
                      ? () => _cpuMiner('restart')
                      : null,
                  child: const Text('应用并重启'),
                ),
                OutlinedButton(
                  onPressed: _enabled && service == 'active'
                      ? () => _cpuMiner('stop')
                      : null,
                  child: const Text('停止 CPU'),
                ),
              ],
            ),
            if (cpu?['active'] is Map)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(
                  '运行中：${(cpu!['active'] as JsonMap)['profile_id']} · ${(cpu['active'] as JsonMap)['threads_percent']}%',
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _pushStatusPanel() => Card(
    color: const Color(0xFF20292E),
    child: Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('通知状态', style: TextStyle(fontWeight: FontWeight.bold)),
          const SizedBox(height: 8),
          _pushStatusLine('服务器凭据', _pushServerStatus),
          _pushStatusLine('手机 Firebase / Token', _pushClientStatus),
          _pushStatusLine('Token API 登记', _pushRegistrationStatus),
        ],
      ),
    ),
  );

  Widget _pushStatusLine(String label, String status) {
    final failed =
        status.contains('失败') ||
        status.contains('未配置') ||
        status.contains('未注册');
    final succeeded =
        status.contains('成功') ||
        status.contains('存在') ||
        status.contains('已登记') ||
        status.contains('已注销');
    final color = failed
        ? Colors.redAccent
        : succeeded
        ? Colors.lightGreenAccent
        : Colors.amber;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Text('$label：$status', style: TextStyle(color: color)),
    );
  }

  Widget _gpuControlCard(
    int gpuId,
    JsonMap? control,
    JsonMap? metrics, {
    bool second = false,
  }) {
    final status = second ? _status2 : _status;
    final enabled = second ? _enabled2 : _enabled;
    final draft = (second ? _gpuDrafts2 : _gpuDrafts).putIfAbsent(
      gpuId,
      _GpuDraft.new,
    );
    final policyDraft = (second ? _policyDrafts2 : _policyDrafts).putIfAbsent(
      gpuId,
      _GpuPolicyDraft.new,
    );
    final policyData =
        (status?['policies'] as JsonMap?)?[gpuId.toString()] as JsonMap?;
    final policy = policyData?['policy'] as JsonMap?;
    final runtime = policyData?['runtime'] as JsonMap?;
    final desired = control?['desired'] as JsonMap?;
    final active = control?['active'] as JsonMap?;
    final catalog = second ? _catalog2 : _catalog;
    final profileId = desired?['profile_id'] ?? draft.profileId;
    final profile = _profileFor(catalog, profileId as String?);
    final supportsIntensity = profile?['miner'] == 'srb-pearl';
    final service = control?['service'] ?? 'unknown';
    final externalPids = control?['external_pids'] as List?;
    final processes = (control?['processes'] as List?)?.whereType<JsonMap>();
    final statusColor = service == 'active'
        ? Colors.green
        : externalPids == null
        ? Colors.orange
        : externalPids.isNotEmpty
        ? Colors.amber
        : Colors.grey;
    final statusLabel = service == 'active'
        ? 'GPU Pocket 正在挖矿'
        : externalPids == null
        ? '进程状态未知'
        : externalPids.isNotEmpty
        ? '其他进程正在使用 · PID ${externalPids.join(', ')}'
        : 'GPU 空闲';
    final minPower = metrics?['power_min_limit_w'] as num?;
    final maxPower = metrics?['power_max_limit_w'] as num?;
    final powerRange = minPower != null && maxPower != null
        ? '允许范围 ${minPower.ceil()}–${maxPower.floor()} W；留空使用算法默认值。'
        : '服务器未提供此卡功耗范围，不能设置功耗上限。';
    final canStage =
        enabled &&
        draft.dirty &&
        draft.profileId != null &&
        draft.poolId != null &&
        draft.wallet.text.trim().isNotEmpty &&
        (draft.powerLimit.text.isEmpty ||
            int.tryParse(draft.powerLimit.text) != null);

    return Card(
      color: statusColor.withAlpha(32),
      margin: const EdgeInsets.only(top: 12),
      child: Padding(
        padding: const EdgeInsets.all(18),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'GPU$gpuId · $service',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const SizedBox(height: 8),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
              decoration: BoxDecoration(
                color: statusColor.withAlpha(70),
                borderRadius: BorderRadius.circular(8),
              ),
              child: Text(
                statusLabel,
                style: TextStyle(
                  color: statusColor == Colors.grey
                      ? Colors.white70
                      : statusColor,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ),
            const SizedBox(height: 16),
            Text(
              'GPU 进程（不显示 Xorg）',
              style: Theme.of(context).textTheme.titleSmall,
            ),
            const SizedBox(height: 4),
            if (processes == null)
              const Text('进程列表暂不可用', style: TextStyle(color: Colors.orange))
            else if (processes.isEmpty)
              const Text('当前没有 GPU 进程')
            else
              for (final process in processes) _gpuProcessRow(process),
            const SizedBox(height: 12),
            if (supportsIntensity) ...[
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: const Text('指定 SRBMiner 算法强度'),
                subtitle: Text(
                  draft.useGpuIntensity
                      ? '启动/重启时使用所选强度。'
                      : '关闭时使用 SRBMiner 默认强度。',
                ),
                value: draft.useGpuIntensity,
                onChanged: enabled
                    ? (value) => setState(() => draft.useGpuIntensity = value)
                    : null,
              ),
              Text(
                '算法强度：${draft.gpuIntensity.round()}（1–31）',
                style: Theme.of(context).textTheme.bodyMedium,
              ),
              Slider(
                min: 1,
                max: 31,
                divisions: 30,
                value: draft.gpuIntensity.clamp(1, 31),
                label: draft.gpuIntensity.round().toString(),
                onChanged: enabled && draft.useGpuIntensity
                    ? (value) => setState(() => draft.gpuIntensity = value)
                    : null,
              ),
            ],
            const SizedBox(height: 12),
            const Text(
              '算法、矿池节点和钱包已统一在“设置”中管理。',
              style: TextStyle(color: Colors.white70),
            ),
            const SizedBox(height: 14),
            TextField(
              controller: draft.powerLimit,
              enabled: enabled && minPower != null && maxPower != null,
              keyboardType: TextInputType.number,
              inputFormatters: [FilteringTextInputFormatter.digitsOnly],
              decoration: InputDecoration(
                labelText: '功耗上限（W，可选）',
                helperText: powerRange,
              ),
              onChanged: (_) => setState(() => draft.dirty = true),
            ),
            const SizedBox(height: 14),
            FilledButton.tonal(
              onPressed: canStage
                  ? () => (second ? _mutateSecond : _mutate)((api) async {
                      await api
                          .request('PUT', '/api/v1/gpus/$gpuId/selection', {
                            'gpu_id': gpuId,
                            'profile_id': draft.profileId,
                            'pool_id': draft.poolId,
                            'wallet': draft.wallet.text.trim(),
                            'power_limit_w': draft.powerLimit.text.isEmpty
                                ? null
                                : int.parse(draft.powerLimit.text),
                            'revision': _revisionFor(gpuId, second: second),
                          });
                      draft.dirty = false;
                    })
                  : null,
              child: const Text('暂存此卡配置'),
            ),
            const SizedBox(height: 10),
            Text(
              '已暂存：${desired?['profile_id'] ?? '无'} / ${desired?['pool_id'] ?? '无'} · ${desired?['power_limit_w'] ?? '默认'} W · 版本 ${desired?['revision'] ?? 0}',
            ),
            Text(
              '运行记录：${active?['profile_id'] ?? '无'} / ${active?['pool_id'] ?? '无'} · ${active?['power_limit_w'] ?? '默认'} W · SRBMiner 强度 ${active?['gpu_intensity'] ?? '默认'} · 版本 ${active?['revision'] ?? 0}',
            ),
            if (desired != null && desired['revision'] != active?['revision'])
              const Padding(
                padding: EdgeInsets.only(top: 6),
                child: Text('暂存配置尚未应用。', style: TextStyle(color: Colors.amber)),
              ),
            const SizedBox(height: 14),
            Wrap(
              spacing: 10,
              runSpacing: 10,
              children: [
                FilledButton(
                  onPressed: enabled && !draft.dirty && desired != null
                      ? () => second
                            ? _secondGpuMiner(gpuId, 'restart')
                            : _miner(gpuId, 'restart')
                      : null,
                  child: Text('应用并重启 GPU$gpuId'),
                ),
                OutlinedButton(
                  onPressed: enabled
                      ? () => second
                            ? _secondGpuMiner(gpuId, 'stop')
                            : _miner(gpuId, 'stop')
                      : null,
                  child: Text('停止 GPU$gpuId'),
                ),
              ],
            ),
            const Divider(height: 32),
            ..._policyControls(
              gpuId,
              policyDraft,
              policy,
              runtime,
              second: second,
            ),
          ],
        ),
      ),
    );
  }

  Widget _gpuProcessRow(JsonMap process) {
    final withinGpuRoot = process['within_gpu_root'] as bool?;
    final path = process['executable'] as String?;
    final location = withinGpuRoot == null
        ? '进程路径无法确认'
        : withinGpuRoot
        ? 'Tongji-Wang/gpu 内'
        : 'GPU 目录外';
    final color = withinGpuRoot == true ? Colors.white70 : Colors.orangeAccent;
    final memory = process['memory_used_mib'] as num?;
    final details = [
      location,
      if (memory != null) '显存 ${memory.toStringAsFixed(0)} MiB',
    ].join(' · ');
    return ListTile(
      dense: true,
      contentPadding: EdgeInsets.zero,
      leading: Icon(
        withinGpuRoot == true ? Icons.memory : Icons.warning_amber_rounded,
        color: color,
      ),
      title: Text(
        '${process['name']} · PID ${process['pid']}',
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
      ),
      subtitle: Text(
        '$details${path == null ? '' : '\n$path'}',
        softWrap: true,
      ),
    );
  }

  List<Widget> _policyControls(
    int gpuId,
    _GpuPolicyDraft draft,
    JsonMap? policy,
    JsonMap? runtime, {
    bool second = false,
  }) {
    final enabled = second ? _enabled2 : _enabled;
    final canSave =
        enabled &&
        draft.dirty &&
        int.tryParse(draft.minFreeMemory.text) != null;
    return [
      Text(
        '自动控制 · 当前 ${policyDataMode(policy, runtime)}',
        style: Theme.of(context).textTheme.titleMedium,
      ),
      const SizedBox(height: 6),
      const Text(
        '自启模式会在 GPU 利用率低于 5% 持续 30 分钟后启动正常挖矿，不降低强度或功耗；深夜模式忽略功耗、计算占用和外部进程，只按可用显存阈值停矿。显存检查每 5 秒一次。',
        style: TextStyle(color: Colors.white60),
      ),
      if (runtime?['message'] is String)
        Padding(
          padding: const EdgeInsets.only(top: 6),
          child: Text(runtime!['message'] as String),
        ),
      SwitchListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('启用此卡自动控制'),
        value: draft.enabled,
        onChanged: enabled
            ? (value) => setState(() {
                draft.enabled = value;
                draft.dirty = true;
              })
            : null,
      ),
      SwitchListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('自启模式：低于 5% 持续 30 分钟后启动'),
        value: draft.throttleEnabled,
        onChanged: enabled
            ? (value) => setState(() {
                draft.throttleEnabled = value;
                draft.dirty = true;
              })
            : null,
      ),
      SwitchListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('深夜模式：北京时间 01:00–08:00'),
        value: draft.nightModeEnabled,
        onChanged: enabled
            ? (value) => setState(() {
                draft.nightModeEnabled = value;
                draft.dirty = true;
              })
            : null,
      ),
      SwitchListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('外部计算任务检测与提醒'),
        value: draft.externalTaskMonitor,
        onChanged: enabled
            ? (value) => setState(() {
                draft.externalTaskMonitor = value;
                draft.dirty = true;
              })
            : null,
      ),
      TextField(
        controller: draft.minFreeMemory,
        enabled: enabled,
        keyboardType: TextInputType.number,
        inputFormatters: [FilteringTextInputFormatter.digitsOnly],
        decoration: const InputDecoration(
          labelText: '最低可用显存（MiB）',
          helperText: '默认 2048 MiB；低于该值时停止自动模式矿工。',
        ),
        onChanged: (_) => setState(() => draft.dirty = true),
      ),
      const SizedBox(height: 12),
      FilledButton.tonal(
        onPressed: canSave
            ? () => (second ? _mutateSecond : _mutate)((api) async {
                await api.request('PUT', '/api/v1/gpus/$gpuId/policy', {
                  'gpu_id': gpuId,
                  'enabled': draft.enabled,
                  'throttle_enabled': draft.throttleEnabled,
                  'night_mode_enabled': draft.nightModeEnabled,
                  'external_task_monitor': draft.externalTaskMonitor,
                  'min_free_memory_mib': int.parse(draft.minFreeMemory.text),
                });
                draft.dirty = false;
              })
            : null,
        child: const Text('保存自动控制设置'),
      ),
    ];
  }

  String policyDataMode(JsonMap? policy, JsonMap? runtime) {
    if (runtime?['memory_blocked'] == true) return '显存保护暂停';
    if (runtime?['external_task'] == true) return '外部任务保护';
    if (policy?['enabled'] != true) return '关闭';
    return '待机 / 自动监控';
  }

  List<Widget> _proxyControls() {
    return [
      const Divider(height: 40),
      Text('代理出口节点', style: Theme.of(context).textTheme.titleLarge),
      const SizedBox(height: 12),
      if (_proxyError != null) _notice(_proxyError!, Colors.amber),
      if (_proxy != null) ...[
        Text(
          '${_proxy!['group']} · ${_proxy!['type']}\n当前：${_proxy!['current'] ?? '未知'}',
        ),
        if (_proxy!['selectable'] != true)
          const Padding(
            padding: EdgeInsets.symmetric(vertical: 12),
            child: Text('当前代理组由服务端自动选择。只有 Selector 组支持手动切换。'),
          ),
        for (final node in (_proxy!['nodes'] as List?) ?? [])
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: Text(node.toString()),
            trailing: node == _proxy!['current']
                ? const Icon(Icons.check_circle, color: Color(0xFF6FE0B7))
                : null,
            onTap: _enabled && _proxy!['selectable'] == true && node != 'DIRECT'
                ? () async {
                    if (await _confirm(
                      '切换代理节点',
                      '将 ${_proxy!['group']} 切换到 $node，已有连接可能需要重新连接。',
                    )) {
                      await _mutate((api) async {
                        await api.request('PUT', '/api/v1/proxy', {
                          'name': node,
                        });
                      });
                    }
                  }
                : null,
          ),
      ],
    ];
  }

  Future<void> _saveSharedMinerConfig() async {
    final pool = _sharedPoolId.text.trim();
    final wallet = _sharedWallet.text.trim();
    if (pool.isEmpty || wallet.isEmpty) {
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('请填写统一矿池节点和钱包 / 账户.worker')));
      return;
    }
    await _storage.write(key: 'miner_shared_pool', value: pool);
    await _storage.write(key: 'miner_shared_wallet', value: wallet);
    if (_sharedGpuProfileId != null) {
      await _storage.write(
        key: 'miner_shared_gpu_profile',
        value: _sharedGpuProfileId,
      );
    }
    if (_sharedCpuProfileId != null) {
      await _storage.write(
        key: 'miner_shared_cpu_profile',
        value: _sharedCpuProfileId,
      );
    }
    setState(() {
      for (final draft in _gpuDrafts.values) {
        draft.profileId = _sharedGpuProfileId ?? draft.profileId;
        draft.poolId = pool;
        _setTextIfChanged(draft.wallet, wallet);
        draft.dirty = true;
      }
      _cpuDraft.profileId = _sharedCpuProfileId ?? _cpuDraft.profileId;
      _cpuDraft.poolId = pool;
      _setTextIfChanged(_cpuDraft.wallet, wallet);
      _cpuDraft.dirty = true;
      _cpuDraft2.profileId = _sharedCpuProfileId ?? _cpuDraft2.profileId;
      _cpuDraft2.poolId = pool;
      _setTextIfChanged(_cpuDraft2.wallet, wallet);
      _cpuDraft2.dirty = true;
    });
    if (mounted) {
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('统一挖矿配置已保存到本机设置')));
    }
  }

  List<Widget> _settings() => [
    _heading('设置', '统一管理挖矿配置、服务器连接与通知。'),
    Text('统一挖矿配置', style: Theme.of(context).textTheme.titleMedium),
    const SizedBox(height: 12),
    DropdownButtonFormField<String>(
      initialValue: _sharedGpuProfileId,
      isExpanded: true,
      decoration: const InputDecoration(labelText: 'GPU 算法 / 矿工'),
      items:
          <String, String>{
                for (final catalog in [_catalog, _catalog2])
                  for (final item
                      in ((catalog?['profiles'] as List?) ?? [])
                          .cast<JsonMap>())
                    item['id'].toString():
                        item['label']?.toString() ?? item['id'].toString(),
              }.entries
              .map(
                (entry) => DropdownMenuItem(
                  value: entry.key,
                  child: Text(entry.value),
                ),
              )
              .toList(),
      onChanged: (value) => setState(() => _sharedGpuProfileId = value),
    ),
    const SizedBox(height: 12),
    DropdownButtonFormField<String>(
      initialValue: _sharedCpuProfileId,
      isExpanded: true,
      decoration: const InputDecoration(labelText: 'CPU 算法'),
      items:
          <String, String>{
                for (final catalog in [_catalog, _catalog2])
                  for (final item
                      in ((catalog?['cpu_profiles'] as List?) ?? [])
                          .cast<JsonMap>())
                    item['id'].toString():
                        item['label']?.toString() ?? item['id'].toString(),
              }.entries
              .map(
                (entry) => DropdownMenuItem(
                  value: entry.key,
                  child: Text(entry.value),
                ),
              )
              .toList(),
      onChanged: (value) => setState(() => _sharedCpuProfileId = value),
    ),
    const SizedBox(height: 12),
    DropdownButtonFormField<String>(
      initialValue: _sharedPoolId.text.isNotEmpty ? _sharedPoolId.text : null,
      isExpanded: true,
      decoration: const InputDecoration(labelText: '统一矿池节点'),
      items:
          <String, String>{
                for (final catalog in [_catalog, _catalog2])
                  for (final item
                      in ((catalog?['profiles'] as List?) ?? [])
                          .cast<JsonMap>())
                    for (final pool
                        in ((item['pools'] as List?) ?? []).cast<JsonMap>())
                      pool['id'].toString():
                          pool['label']?.toString() ?? pool['id'].toString(),
                for (final catalog in [_catalog, _catalog2])
                  for (final item
                      in ((catalog?['cpu_profiles'] as List?) ?? [])
                          .cast<JsonMap>())
                    for (final pool
                        in ((item['pools'] as List?) ?? []).cast<JsonMap>())
                      pool['id'].toString():
                          pool['label']?.toString() ?? pool['id'].toString(),
              }.entries
              .map(
                (entry) => DropdownMenuItem(
                  value: entry.key,
                  child: Text(entry.value),
                ),
              )
              .toList(),
      onChanged: (value) => setState(() {
        _sharedPoolId.text = value ?? '';
      }),
    ),
    const SizedBox(height: 12),
    Builder(
      builder: (context) {
        final wallets = <String>{};
        for (final status in [_status, _status2]) {
          final cpu = status?['cpu_mining'] as JsonMap?;
          for (final key in ['desired', 'active']) {
            final selection = cpu?[key] as JsonMap?;
            final wallet = selection?['wallet']?.toString();
            if (wallet != null && wallet.isNotEmpty) wallets.add(wallet);
          }
          for (final control
              in ((status?['gpu_controls'] as Map?) ?? {}).values) {
            if (control is Map) {
              for (final key in ['desired', 'active']) {
                final selection = control[key];
                if (selection is Map) {
                  final wallet = selection['wallet']?.toString();
                  if (wallet != null && wallet.isNotEmpty) wallets.add(wallet);
                }
              }
            }
          }
        }
        return DropdownButtonFormField<String>(
          initialValue: wallets.contains(_sharedWallet.text)
              ? _sharedWallet.text
              : null,
          isExpanded: true,
          decoration: const InputDecoration(labelText: '可选钱包 / 账户'),
          items: wallets
              .map(
                (wallet) =>
                    DropdownMenuItem(value: wallet, child: Text(wallet)),
              )
              .toList(),
          onChanged: (value) =>
              setState(() => _setTextIfChanged(_sharedWallet, value ?? '')),
        );
      },
    ),
    const SizedBox(height: 12),
    TextField(
      controller: _sharedWallet,
      autocorrect: false,
      enableSuggestions: false,
      decoration: const InputDecoration(
        labelText: '钱包 / 账户.worker（公开，可直接显示）',
        helperText: '不要填助记词或私钥。',
      ),
    ),
    const SizedBox(height: 12),
    FilledButton.tonal(
      onPressed: _saveSharedMinerConfig,
      child: const Text('保存统一挖矿配置'),
    ),
    const SizedBox(height: 24),
    Text('服务器连接', style: Theme.of(context).textTheme.titleMedium),
    const SizedBox(height: 12),
    TextField(
      controller: _url,
      keyboardType: TextInputType.url,
      autocorrect: false,
      enabled: !_working && !_loading,
      decoration: const InputDecoration(
        labelText: '服务地址',
        hintText: 'https://gpu.example.com',
      ),
    ),
    const SizedBox(height: 16),
    TextField(
      controller: _token,
      obscureText: true,
      autocorrect: false,
      enableSuggestions: false,
      enabled: !_working && !_loading,
      decoration: const InputDecoration(
        labelText: 'API 令牌',
        helperText: '保存在系统安全存储中。',
      ),
    ),
    const SizedBox(height: 20),
    FilledButton(
      onPressed: !_working && !_loading ? _connect : null,
      child: const Text('保存并连接主服务器'),
    ),
    const SizedBox(height: 24),
    const Divider(),
    const SizedBox(height: 16),
    Text('第二台服务器', style: Theme.of(context).textTheme.titleMedium),
    const SizedBox(height: 12),
    TextField(
      controller: _url2,
      keyboardType: TextInputType.url,
      autocorrect: false,
      enabled: !_working && !_loading,
      decoration: const InputDecoration(labelText: '第二台服务地址'),
    ),
    const SizedBox(height: 16),
    TextField(
      controller: _token2,
      obscureText: true,
      autocorrect: false,
      enableSuggestions: false,
      enabled: !_working && !_loading,
      decoration: const InputDecoration(labelText: '第二台 API 令牌'),
    ),
    const SizedBox(height: 20),
    FilledButton(
      onPressed: !_working && !_loading ? _connectSecond : null,
      child: const Text('保存并连接第二台服务器'),
    ),
    const SizedBox(height: 12),
    TextButton(
      onPressed: !_working && !_loading
          ? () async {
              await _unregisterPush(_api);
              await _storage.delete(key: 'server_url');
              await _storage.delete(key: 'server_token');
              await _storage.delete(key: 'server2_url');
              await _storage.delete(key: 'server2_token');
              _api?.close();
              _api2?.close();
              _api = null;
              _api2 = null;
              _generation++;
              for (final draft in _gpuDrafts.values) {
                draft.dispose();
              }
              _gpuDrafts.clear();
              for (final draft in _policyDrafts.values) {
                draft.dispose();
              }
              _policyDrafts.clear();
              if (mounted) {
                setState(() {
                  _token.clear();
                  _url.clear();
                  _token2.clear();
                  _url2.clear();
                  _cpuDraft2
                    ..dirty = false
                    ..profileId = null
                    ..poolId = null
                    ..threadsPercent = 50
                    ..wallet.clear();
                  _pushServerStatus = '等待服务器状态';
                  _pushClientStatus = '尚未初始化';
                  _pushRegistrationStatus = '尚未提交设备令牌';
                  _status = null;
                  _status2 = null;
                  _catalog = null;
                  _catalog2 = null;
                  _proxy = null;
                  _proxyError = null;
                  _lastSuccess = null;
                  _error = null;
                });
              }
            }
          : null,
      child: const Text('删除连接与本机凭据'),
    ),
    const SizedBox(height: 20),
    const Text(
      'IPv6 示例：https://[2001:db8::1]:443\nHTTP 仅用于本机调试或 SSH 回环转发。',
      style: TextStyle(color: Colors.white60),
    ),
  ];
}
