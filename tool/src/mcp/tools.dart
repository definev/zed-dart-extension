import 'dart:convert';
import 'dart:io';

class FlutterToolsToolRegistry {
  List<Map<String, Object?>> listTools() {
    return [
      _tool(
        'flutter_devices',
        'List available Flutter devices, optionally filtered by query.',
        {
          'type': 'object',
          'additionalProperties': false,
          'properties': {
            'query': {
              'type': 'string',
              'description': 'Optional substring filter for id, name, platform, or category.',
            },
          },
        },
      ),
      _tool(
        'flutter_devtools_url',
        'Build a local Flutter DevTools URL from a VM service URI.',
        {
          'type': 'object',
          'additionalProperties': false,
          'required': ['vm_service_uri'],
          'properties': {
            'vm_service_uri': {
              'type': 'string',
              'description': 'VM service URI such as ws://127.0.0.1:12345/ws.',
            },
          },
        },
      ),
      _tool(
        'flutter_vm_probe',
        'Connect to a Flutter VM service and call getVM.',
        {
          'type': 'object',
          'additionalProperties': false,
          'required': ['vm_service_uri'],
          'properties': {
            'vm_service_uri': {
              'type': 'string',
              'description': 'VM service URI such as ws://127.0.0.1:12345/ws.',
            },
          },
        },
      ),
      _tool(
        'flutter_hot_reload',
        'Trigger Flutter hot reload through the same shell-backed attach shim used by the extension slash command.',
        _hotActionSchema(),
      ),
      _tool(
        'flutter_hot_restart',
        'Trigger Flutter hot restart through the same shell-backed attach shim used by the extension slash command.',
        _hotActionSchema(),
      ),
    ];
  }

  Map<String, Object?> _tool(
    String name,
    String description,
    Map<String, Object?> inputSchema,
  ) {
    return {
      'name': name,
      'description': description,
      'inputSchema': inputSchema,
    };
  }

  Map<String, Object?> _hotActionSchema() {
    return {
      'type': 'object',
      'additionalProperties': false,
      'properties': {
        'workspace_root': {
          'type': 'string',
          'description': 'Workspace root used to resolve relative VM service info files and auto-discover the latest generated file.',
        },
        'vm_service_uri_or_info_file': {
          'type': 'string',
          'description': 'Optional VM service URI, host:port, or JSON info file path. If omitted, the latest generated file under .zed/dart/vmservice-info is used.',
        },
      },
    };
  }

  Future<Map<String, Object?>> callTool(String name, Object? arguments) async {
    final args = arguments is Map<String, dynamic>
        ? arguments
        : <String, dynamic>{};

    try {
      switch (name) {
        case 'flutter_devices':
          return _success(await _flutterDevices(args['query'] as String? ?? ''));
        case 'flutter_devtools_url':
          return _success(_flutterDevtoolsUrl(_requiredString(args, 'vm_service_uri')));
        case 'flutter_vm_probe':
          return _success(await _flutterVmProbe(_requiredString(args, 'vm_service_uri')));
        case 'flutter_hot_reload':
          return _success(await _flutterHotAction(
            workspaceRoot: args['workspace_root'] as String?,
            vmServiceUriOrInfoFile: args['vm_service_uri_or_info_file'] as String? ?? '',
            actionKey: 'r',
            actionLabel: 'hot reload',
          ));
        case 'flutter_hot_restart':
          return _success(await _flutterHotAction(
            workspaceRoot: args['workspace_root'] as String?,
            vmServiceUriOrInfoFile: args['vm_service_uri_or_info_file'] as String? ?? '',
            actionKey: 'R',
            actionLabel: 'hot restart',
          ));
        default:
          throw ArgumentError('Unknown tool `$name`');
      }
    } catch (error) {
      return _error(error.toString());
    }
  }

  Map<String, Object?> _success(String text) {
    return {
      'content': [
        {
          'type': 'text',
          'text': text,
        },
      ],
    };
  }

  Map<String, Object?> _error(String text) {
    return {
      'content': [
        {
          'type': 'text',
          'text': text,
        },
      ],
      'isError': true,
    };
  }
}

String _requiredString(Map<String, dynamic> args, String key) {
  final value = args[key];
  if (value is String && value.trim().isNotEmpty) {
    return value;
  }
  throw ArgumentError('`$key` is required');
}

Future<String> _flutterDevices(String query) async {
  final result = await Process.run('flutter', ['devices', '--machine']);
  if (result.exitCode != 0) {
    throw StateError(_failureMessage('Failed to list Flutter devices', result));
  }

  final parsed = jsonDecode(result.stdout as String);
  if (parsed is! List) {
    throw const FormatException('Expected flutter devices output to be a JSON array');
  }

  final devices = parsed
      .cast<Map>()
      .map((device) => _FlutterDevice(
            id: device['id'] as String? ?? '',
            name: device['name'] as String? ?? '',
            platform: device['platformType'] as String?,
            emulator: device['emulator'] as bool?,
            category: device['category'] as String?,
          ))
      .where((device) => device.id.isNotEmpty && device.name.isNotEmpty)
      .toList();

  final filtered = _filterDevices(devices, query);
  if (filtered.isEmpty) {
    return 'No Flutter devices found.';
  }

  return filtered.map((device) => '- ${device.description}').join('\n');
}

List<_FlutterDevice> _filterDevices(List<_FlutterDevice> devices, String query) {
  final normalized = query.trim().toLowerCase();
  if (normalized.isEmpty) {
    return devices;
  }

  return devices.where((device) {
    return device.id.toLowerCase().contains(normalized) ||
        device.name.toLowerCase().contains(normalized) ||
        (device.platform?.toLowerCase().contains(normalized) ?? false) ||
        (device.category?.toLowerCase().contains(normalized) ?? false);
  }).toList();
}

String _flutterDevtoolsUrl(String vmServiceUri) {
  final normalized = _normalizeVmServiceUri(vmServiceUri);
  return 'http://127.0.0.1:9100?uri=${Uri.encodeComponent(normalized)}';
}

Future<String> _flutterVmProbe(String vmServiceUri) async {
  final websocketUri = _vmServiceWebsocketUri(vmServiceUri);
  final uri = Uri.parse(websocketUri);
  final socket = await WebSocket.connect(uri.toString());
  try {
    socket.add(jsonEncode({
      'jsonrpc': '2.0',
      'id': 'zed-dart-probe',
      'method': 'getVM',
    }));

    final response = await socket.first.timeout(const Duration(seconds: 10));
    final payload = switch (response) {
      String text => text,
      List<int> bytes => utf8.decode(bytes),
      _ => throw StateError(
          'Unexpected VM service response from `$websocketUri`: $response',
        ),
    };

    final parsed = jsonDecode(payload);
    return const JsonEncoder.withIndent('  ').convert(parsed);
  } finally {
    await socket.close();
  }
}

Future<String> _flutterHotAction({
  required String? workspaceRoot,
  required String vmServiceUriOrInfoFile,
  required String actionKey,
  required String actionLabel,
}) async {
  final resolved = _resolveHotTarget(workspaceRoot, vmServiceUriOrInfoFile);
  final projectRoot = workspaceRoot?.trim();
  if (projectRoot == null || projectRoot.isEmpty) {
    throw ArgumentError(
      '`workspace_root` is required for flutter_hot_reload and flutter_hot_restart',
    );
  }

  final script = _buildFlutterAttachShellScript(
    flutter: 'flutter',
    projectRoot: projectRoot,
    vmServiceUri: resolved.uri,
    actionKey: actionKey,
    actionLabel: actionLabel,
  );

  final result = await Process.run(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-Command', script],
    workingDirectory: projectRoot,
  );
  if (result.exitCode != 0) {
    throw StateError(
      _failureMessage('Failed to trigger Flutter $actionLabel', result),
    );
  }

  final output = (result.stdout as String).trim();
  return output.isEmpty ? resolved.source : '${resolved.source}\n$output';
}

_ResolvedHotTarget _resolveHotTarget(String? workspaceRoot, String input) {
  final trimmed = input.trim();
  if (trimmed.isEmpty) {
    final root = _requireWorkspaceRoot(workspaceRoot);
    final latestInfo = _discoverLatestVmServiceInfoFile(root);
    return _ResolvedHotTarget(
      uri: _readVmServiceUriFromInfoFile(latestInfo),
      source: 'Using VM service info file `${latestInfo.path}`',
    );
  }

  final pathStyle = trimmed.endsWith('.json') ||
      trimmed.contains(r'\') ||
      trimmed.contains('/') ||
      trimmed.startsWith('.');
  if (pathStyle) {
    final root = _requireWorkspaceRoot(workspaceRoot);
    final candidate = File(trimmed);
    final file = candidate.isAbsolute
        ? candidate
        : File('${root.path}${Platform.pathSeparator}$trimmed');
    return _ResolvedHotTarget(
      uri: _readVmServiceUriFromInfoFile(file),
      source: 'Using VM service info file `${file.path}`',
    );
  }

  return _ResolvedHotTarget(
    uri: _normalizeVmServiceUri(trimmed),
    source: 'Using provided VM service URI',
  );
}

Directory _requireWorkspaceRoot(String? workspaceRoot) {
  final trimmed = workspaceRoot?.trim();
  if (trimmed == null || trimmed.isEmpty) {
    throw ArgumentError(
      '`workspace_root` is required when resolving VM service info files',
    );
  }
  return Directory(trimmed);
}

File _discoverLatestVmServiceInfoFile(Directory workspaceRoot) {
  final directory = Directory(
    '${workspaceRoot.path}${Platform.pathSeparator}.zed'
    '${Platform.pathSeparator}dart'
    '${Platform.pathSeparator}vmservice-info',
  );
  if (!directory.existsSync()) {
    throw StateError(
      'Failed to read VM service info directory `${directory.path}`. '
      'Start a Flutter debug launch first or provide a VM service URI.',
    );
  }

  final files = directory
      .listSync()
      .whereType<File>()
      .where((file) => file.path.endsWith('.json'))
      .toList()
    ..sort((left, right) {
      final leftTime = left.statSync().modified;
      final rightTime = right.statSync().modified;
      return rightTime.compareTo(leftTime);
    });

  if (files.isEmpty) {
    throw StateError(
      'No VM service info files found in `${directory.path}`. '
      'Start a Flutter debug launch first or provide a VM service URI.',
    );
  }

  return files.first;
}

String _readVmServiceUriFromInfoFile(File file) {
  final contents = file.readAsStringSync();
  final value = jsonDecode(contents);
  if (value is! Map<String, dynamic>) {
    throw FormatException(
      'Invalid VM service info JSON in `${file.path}`: expected an object',
    );
  }

  for (final key in ['uri', 'vmServiceUri', 'wsUri', 'ws_uri']) {
    final candidate = value[key];
    if (candidate is String && candidate.trim().isNotEmpty) {
      return _normalizeVmServiceUri(candidate);
    }
  }

  throw StateError(
    'VM service info file `${file.path}` does not contain a supported URI field',
  );
}

String _normalizeVmServiceUri(String vmServiceUri) {
  final trimmed = vmServiceUri.trim();
  if (trimmed.isEmpty) {
    throw ArgumentError('VM service URI is required');
  }

  if (trimmed.startsWith('ws://') ||
      trimmed.startsWith('wss://') ||
      trimmed.startsWith('http://') ||
      trimmed.startsWith('https://')) {
    return trimmed;
  }

  return 'http://$trimmed';
}

String _vmServiceWebsocketUri(String vmServiceUri) {
  final normalized = _normalizeVmServiceUri(vmServiceUri);
  if (normalized.startsWith('ws://') || normalized.startsWith('wss://')) {
    return normalized;
  }

  if (normalized.startsWith('http://')) {
    return 'ws://${_ensureVmServiceWsPath(normalized.substring('http://'.length))}';
  }

  if (normalized.startsWith('https://')) {
    return 'wss://${_ensureVmServiceWsPath(normalized.substring('https://'.length))}';
  }

  throw ArgumentError(
    'Unsupported VM service URI `$normalized`. Expected ws://, wss://, http://, or https://',
  );
}

String _ensureVmServiceWsPath(String uriWithoutScheme) {
  if (uriWithoutScheme.endsWith('/ws') || uriWithoutScheme.endsWith('/ws/')) {
    return uriWithoutScheme.replaceFirst(RegExp(r'/$'), '');
  }
  if (uriWithoutScheme.endsWith('/')) {
    return '${uriWithoutScheme}ws';
  }
  if (uriWithoutScheme.contains('/')) {
    return uriWithoutScheme;
  }
  return '$uriWithoutScheme/ws';
}

String _buildFlutterAttachShellScript({
  required String flutter,
  required String projectRoot,
  required String vmServiceUri,
  required String actionKey,
  required String actionLabel,
}) {
  String quote(String value) => "'${value.replaceAll("'", "''")}'";

  return [
    r'$psi = New-Object System.Diagnostics.ProcessStartInfo;',
    '\$psi.FileName = ${quote(flutter)};',
    '\$psi.Arguments = \'attach --debug-uri \' + ${quote(vmServiceUri)} + '
        '\' --project-root \' + ${quote(projectRoot)} + \' --report-ready\';',
    '\$psi.WorkingDirectory = ${quote(projectRoot)};',
    r'$psi.UseShellExecute = $false;',
    r'$psi.RedirectStandardInput = $true;',
    r'$psi.RedirectStandardOutput = $true;',
    r'$psi.RedirectStandardError = $true;',
    r'$p = New-Object System.Diagnostics.Process;',
    r'$p.StartInfo = $psi;',
    r'$stdout = New-Object System.Text.StringBuilder;',
    r'$stderr = New-Object System.Text.StringBuilder;',
    r'$ready = New-Object System.Threading.ManualResetEventSlim($false);',
    r'$stderrReady = New-Object System.Threading.ManualResetEventSlim($false);',
    r'$handler = [System.Diagnostics.DataReceivedEventHandler]{',
    r' param($sender, $eventArgs)',
    r' if ($null -ne $eventArgs.Data) {',
    r'  [void]$stdout.AppendLine($eventArgs.Data);',
    r"  if ($eventArgs.Data -match 'Flutter run key commands|The Flutter DevTools debugger and profiler|ready|A Dart VM Service') { $ready.Set() }",
    r' }',
    r'};',
    r'$errorHandler = [System.Diagnostics.DataReceivedEventHandler]{',
    r' param($sender, $eventArgs)',
    r' if ($null -ne $eventArgs.Data) {',
    r'  [void]$stderr.AppendLine($eventArgs.Data);',
    r"  if ($eventArgs.Data -match 'Flutter run key commands|The Flutter DevTools debugger and profiler|ready|A Dart VM Service') { $stderrReady.Set() }",
    r' }',
    r'};',
    r'$p.add_OutputDataReceived($handler);',
    r'$p.add_ErrorDataReceived($errorHandler);',
    r'if (-not $p.Start()) { throw ''Failed to start flutter attach.'' };',
    r'$p.BeginOutputReadLine();',
    r'$p.BeginErrorReadLine();',
    r'if (-not ($ready.Wait(20000) -or $stderrReady.Wait(20000))) {',
    r' try { if (-not $p.HasExited) { $p.Kill() } } catch {};',
    r" throw 'Timed out waiting for flutter attach to become ready.'",
    r'};',
    '\$p.StandardInput.WriteLine(${quote(actionKey)});',
    r'$p.StandardInput.WriteLine(''q'');',
    r'$p.StandardInput.Flush();',
    r'if (-not $p.WaitForExit(30000)) {',
    r' try { $p.Kill() } catch {};',
    " throw 'Timed out waiting for flutter attach to exit after $actionLabel.'",
    r'};',
    r'$combined = (($stdout.ToString() + "`n" + $stderr.ToString()).Trim());',
    r"if ($p.ExitCode -ne 0 -and $combined -notmatch 'ready|Reloaded|Restarted|performing hot reload|performing hot restart') {",
    r" throw ('flutter attach exited with code ' + $p.ExitCode + ': ' + $combined)",
    r'};',
    "Write-Output ('Flutter $actionLabel requested via shell attach shim.' + [Environment]::NewLine + \$combined.Trim())",
  ].join();
}

String _failureMessage(String prefix, ProcessResult result) {
  final stderr = (result.stderr as String).trim();
  final stdout = (result.stdout as String).trim();
  final detail = stderr.isNotEmpty ? stderr : stdout;
  return detail.isEmpty ? prefix : '$prefix: $detail';
}

class _FlutterDevice {
  _FlutterDevice({
    required this.id,
    required this.name,
    this.platform,
    this.emulator,
    this.category,
  });

  final String id;
  final String name;
  final String? platform;
  final bool? emulator;
  final String? category;

  String get description {
    final parts = <String>['$name (`$id`)'];
    if (platform != null && platform!.isNotEmpty) {
      parts.add(platform!);
    }
    if (category != null && category!.isNotEmpty) {
      parts.add(category!);
    }
    if (emulator == true) {
      parts.add('emulator');
    }
    return parts.join(' | ');
  }
}

class _ResolvedHotTarget {
  _ResolvedHotTarget({
    required this.uri,
    required this.source,
  });

  final String uri;
  final String source;
}
