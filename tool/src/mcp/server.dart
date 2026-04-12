import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'tools.dart';

Future<void> runFlutterToolsMcpServer() async {
  final server = _McpServer(stdin, stdout, FlutterToolsToolRegistry());
  await server.run();
}

class _McpServer {
  _McpServer(this._input, this._output, this._toolRegistry);

  final Stream<List<int>> _input;
  final IOSink _output;
  final FlutterToolsToolRegistry _toolRegistry;
  final _buffer = BytesBuilder(copy: false);

  Future<void> run() async {
    await for (final chunk in _input) {
      _buffer.add(chunk);
      while (await _drainMessage()) {}
    }
  }

  Future<bool> _drainMessage() async {
    final bytes = _stripLeadingBom(_buffer.toBytes());
    final boundary = _findHeaderBoundary(bytes);
    if (boundary == null) {
      return false;
    }

    final headerText = ascii.decode(bytes.sublist(0, boundary.headerEnd));
    final contentLength = _parseContentLength(headerText);
    final messageStart = boundary.messageStart;
    final messageEnd = messageStart + contentLength;
    if (bytes.length < messageEnd) {
      return false;
    }

    final body = utf8.decode(bytes.sublist(messageStart, messageEnd));
    final remaining = bytes.sublist(messageEnd);
    _buffer.clear();
    _buffer.add(remaining);

    await _handleMessage(body);
    return true;
  }

  List<int> _stripLeadingBom(List<int> bytes) {
    if (bytes.length >= 3 &&
        bytes[0] == 0xef &&
        bytes[1] == 0xbb &&
        bytes[2] == 0xbf) {
      return bytes.sublist(3);
    }
    return bytes;
  }

  _HeaderBoundary? _findHeaderBoundary(List<int> bytes) {
    for (var i = 0; i <= bytes.length - 4; i++) {
      if (bytes[i] == 13 &&
          bytes[i + 1] == 10 &&
          bytes[i + 2] == 13 &&
          bytes[i + 3] == 10) {
        return _HeaderBoundary(headerEnd: i, messageStart: i + 4);
      }
    }

    for (var i = 0; i <= bytes.length - 2; i++) {
      if (bytes[i] == 10 && bytes[i + 1] == 10) {
        return _HeaderBoundary(headerEnd: i, messageStart: i + 2);
      }
    }

    return null;
  }

  int _parseContentLength(String headerText) {
    for (final line in headerText.split('\r\n')) {
      final separator = line.indexOf(':');
      if (separator == -1) {
        continue;
      }

      final name = line.substring(0, separator).trim().toLowerCase();
      if (name != 'content-length') {
        continue;
      }

      final value = line.substring(separator + 1).trim();
      return int.parse(value);
    }

    throw const FormatException('Missing Content-Length header');
  }

  Future<void> _handleMessage(String body) async {
    final message = jsonDecode(body);
    if (message is! Map<String, dynamic>) {
      return;
    }

    final id = message['id'];
    final method = message['method'];
    if (method is! String) {
      return;
    }

    if (id == null) {
      await _handleNotification(method, message['params']);
      return;
    }

    try {
      final result = await _handleRequest(method, message['params']);
      _send({'jsonrpc': '2.0', 'id': id, 'result': result});
    } catch (error, stackTrace) {
      stderr.writeln('MCP request failed for $method: $error\n$stackTrace');
      _send({
        'jsonrpc': '2.0',
        'id': id,
        'error': {'code': -32000, 'message': error.toString()},
      });
    }
  }

  Future<void> _handleNotification(String method, Object? params) async {
    switch (method) {
      case 'notifications/initialized':
      case 'initialized':
        return;
    }
  }

  Future<Object?> _handleRequest(String method, Object? params) async {
    switch (method) {
      case 'initialize':
        return {
          'protocolVersion': '2025-03-26',
          'capabilities': {'tools': <String, Object?>{}},
          'serverInfo': {'name': 'zed-dart-flutter-tools', 'version': '0.1.0'},
        };
      case 'ping':
        return <String, Object?>{};
      case 'tools/list':
        return {'tools': _toolRegistry.listTools()};
      case 'tools/call':
        final request = params is Map<String, dynamic>
            ? params
            : <String, dynamic>{};
        return _toolRegistry.callTool(
          request['name'] as String? ?? '',
          request['arguments'],
        );
      default:
        throw UnsupportedError('Unsupported MCP method `$method`');
    }
  }

  void _send(Map<String, Object?> payload) {
    final encoded = utf8.encode(jsonEncode(payload));
    _output.add(ascii.encode('Content-Length: ${encoded.length}\r\n\r\n'));
    _output.add(encoded);
    _output.flush();
  }
}

class _HeaderBoundary {
  const _HeaderBoundary({
    required this.headerEnd,
    required this.messageStart,
  });

  final int headerEnd;
  final int messageStart;
}
