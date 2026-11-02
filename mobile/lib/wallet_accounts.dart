const walletAccountPresets = <String, String>{
  'Kryptex': 'krxXPVDK8N',
  'NiceHash': 'NHbHUa4Ew63dCUt3JvLFCTLPbaQxAZJKnWQX',
};

String? walletPresetFor(String wallet) {
  for (final account in walletAccountPresets.values) {
    if (wallet == account || wallet.startsWith('$account.')) return account;
  }
  return null;
}

String walletWithPresetAccount(String wallet, String account) {
  final workerIndex = wallet.indexOf('.');
  final worker = workerIndex == -1 ? '' : wallet.substring(workerIndex);
  return '$account$worker';
}

String walletForDevice(
  String sharedWallet,
  String existingWallet,
  String? presetAccount,
) {
  if (presetAccount == null) return sharedWallet;
  final existingWorkerIndex = existingWallet.indexOf('.');
  final sharedWorkerIndex = sharedWallet.indexOf('.');
  final worker = existingWorkerIndex == -1
      ? sharedWorkerIndex == -1
            ? ''
            : sharedWallet.substring(sharedWorkerIndex)
      : existingWallet.substring(existingWorkerIndex);
  return '$presetAccount$worker';
}
