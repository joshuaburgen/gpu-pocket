import 'package:flutter_test/flutter_test.dart';
import 'package:gpu_pocket/api.dart';

void main() {
  test('accepts HTTPS domain and IPv6 plus local debug forwarding', () {
    expect(validateBaseUrl('https://gpu.example.com').host, 'gpu.example.com');
    expect(validateBaseUrl('https://[2001:db8::1]:8443').port, 8443);
    expect(validateBaseUrl('http://127.0.0.1:8787').port, 8787);
  });
  test('rejects public cleartext credentials query and paths', () {
    for (final value in [
      'http://gpu.example.com',
      'http://100.94.37.27:8787',
      'https://u:pass@gpu.example.com',
      'https://gpu.example.com/api',
      'https://gpu.example.com?token=secret',
    ]) {
      expect(() => validateBaseUrl(value), throwsA(isA<ApiException>()));
    }
  });
}
