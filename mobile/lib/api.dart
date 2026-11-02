import 'dart:async';
import 'dart:convert';
import 'dart:io';

typedef JsonMap = Map<String, dynamic>;

class ApiException implements Exception {
  ApiException(this.message);
  final String message;
  @override
  String toString() => message;
}

Uri validateBaseUrl(String input) {
  final uri = Uri.parse(input.trim());
  final local = ['127.0.0.1', 'localhost', '::1'].contains(uri.host);
  if (uri.host.isEmpty ||
      uri.userInfo.isNotEmpty ||
      uri.hasQuery ||
      uri.hasFragment ||
      (uri.path.isNotEmpty && uri.path != '/') ||
      (uri.scheme != 'https' && !(uri.scheme == 'http' && local))) {
    throw ApiException('请输入 HTTPS 地址；HTTP 仅允许本机回环地址。IPv6 地址需放在 [] 内。');
  }
  return uri.replace(path: '');
}

class GpuApi {
  GpuApi(String baseUrl, this.token) : base = validateBaseUrl(baseUrl);
  final Uri base;
  final String token;
  final HttpClient _client = HttpClient()
    ..connectionTimeout = const Duration(seconds: 8);

  Future<JsonMap> request(String method, String path, [JsonMap? body]) async {
    if (token.trim().length < 32) throw ApiException('令牌至少需要 32 个字符。');
    try {
      return await _request(
        method,
        path,
        body,
      ).timeout(const Duration(seconds: 15));
    } on TimeoutException {
      throw ApiException('连接超时，请检查服务器与网络。');
    } on SocketException {
      throw ApiException('连接失败，请检查地址、DNS 和隧道状态。');
    } on HandshakeException {
      throw ApiException('HTTPS 证书验证失败。');
    }
  }

  Future<JsonMap> _request(String method, String path, JsonMap? body) async {
    final request = await _client.openUrl(method, base.replace(path: path));
    // Never forward bearer credentials across redirects, including Access login pages.
    request.followRedirects = false;
    request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $token');
    if (body != null) {
      request.headers.contentType = ContentType.json;
      request.write(jsonEncode(body));
    }
    final response = await request.close();
    final chunks = <int>[];
    await for (final chunk in response) {
      chunks.addAll(chunk);
      if (chunks.length > 1024 * 1024) throw ApiException('服务器响应过大。');
    }
    JsonMap data;
    try {
      data = jsonDecode(utf8.decode(chunks)) as JsonMap;
    } on FormatException {
      final body = utf8
          .decode(chunks, allowMalformed: true)
          .replaceAll(RegExp(r'\s+'), ' ')
          .trim();
      final preview = body.length > 180 ? '${body.substring(0, 180)}…' : body;
      final contentType = response.headers.contentType?.mimeType ?? '未知';
      throw ApiException(
        '服务器返回了非 JSON 响应（HTTP ${response.statusCode}，$contentType）'
        '${preview.isEmpty ? '。' : '：$preview'}',
      );
    } on TypeError {
      throw ApiException('服务器响应格式不正确。');
    }
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw ApiException(
        'HTTP ${response.statusCode}: ${data['error'] ?? '请求失败'}',
      );
    }
    return data;
  }

  void close() => _client.close(force: true);
}
