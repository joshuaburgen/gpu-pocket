import 'package:flutter_test/flutter_test.dart';
import 'package:gpu_pocket/wallet_accounts.dart';

void main() {
  test('preset selection replaces the account and keeps the worker suffix', () {
    expect(
      walletWithPresetAccount('old-account.3080ti-gpu0', 'NH-account'),
      'NH-account.3080ti-gpu0',
    );
    expect(walletPresetFor('krxXPVDK8N.worker'), 'krxXPVDK8N');
    expect(
      walletPresetFor('NHbHUa4Ew63dCUt3JvLFCTLPbaQxAZJKnWQX.worker'),
      'NHbHUa4Ew63dCUt3JvLFCTLPbaQxAZJKnWQX',
    );
  });

  test('preset save preserves each device worker name', () {
    expect(
      walletForDevice(
        'NH-account.shared-worker',
        'old-account.3080ti-gpu0',
        'NH-account',
      ),
      'NH-account.3080ti-gpu0',
    );
    expect(
      walletForDevice('NH-account.shared-worker', '', 'NH-account'),
      'NH-account.shared-worker',
    );
  });

  test('manual account input remains unchanged', () {
    expect(
      walletForDevice('custom-account.worker', 'old-account.gpu0', null),
      'custom-account.worker',
    );
  });
}
