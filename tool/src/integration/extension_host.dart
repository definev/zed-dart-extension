import 'dart:convert';
import 'dart:io';

import 'commands.dart';
import 'common.dart';

File? _cachedExtensionWasm;
bool _preparedExtensionWasm = false;

Future<File> prepareExtensionWasm() async {
  if (_preparedExtensionWasm) {
    final cached = _cachedExtensionWasm;
    if (cached != null && await cached.exists()) {
      return cached;
    }
  }

  final wasm = await _buildExtensionWasm();
  _preparedExtensionWasm = true;
  return wasm;
}

Future<void> installExtension(Directory userDataDir, File wasmPath) async {
  final extensionDir = Directory(
    joinPath(userDataDir.path, 'extensions', 'installed', extensionId),
  );
  if (await extensionDir.exists()) {
    await extensionDir.delete(recursive: true);
  }
  await extensionDir.create(recursive: true);

  await File(
    joinPath(extensionDir.path, 'extension.toml'),
  ).writeAsString(await File('extension.toml').readAsString());
  await wasmPath.copy(joinPath(extensionDir.path, 'extension.wasm'));
  await copyDirectory(
    Directory('languages'),
    Directory(joinPath(extensionDir.path, 'languages')),
  );
  await copyDirectory(
    Directory('debug_adapter_schemas'),
    Directory(joinPath(extensionDir.path, 'debug_adapter_schemas')),
  );
  await _writeExtensionIndex(userDataDir);
}

Future<void> launchZedForCheck(Directory userDataDir) async {
  final zedPath = resolveZedExecutable();
  await Process.start(zedPath, ['--user-data-dir', userDataDir.path]);
}

Future<Process> launchZedForManualUse(Directory userDataDir) {
  final zedPath = resolveZedExecutable();
  return Process.start(zedPath, ['--user-data-dir', userDataDir.path]);
}

Future<void> waitForZedProfileStartup(Directory userDataDir) async {
  final logFile = File(joinPath(userDataDir.path, 'logs', 'Zed.log'));
  final deadline = DateTime.now().add(const Duration(seconds: 15));

  while (DateTime.now().isBefore(deadline)) {
    if (await logFile.exists()) {
      return;
    }
    await Future<void>.delayed(const Duration(milliseconds: 500));
  }

  throw IntegrationFailure(
    'Zed did not initialize the isolated profile at `${userDataDir.path}`.\n'
    'No log file was created at `${logFile.path}`.\n'
    'The app launch completed without creating profile log output.',
  );
}

Future<void> verifyExtensionRegistered(Directory userDataDir) async {
  final indexFile = File(
    joinPath(userDataDir.path, 'extensions', 'index.json'),
  );
  final logFile = File(joinPath(userDataDir.path, 'logs', 'Zed.log'));

  final deadline = DateTime.now().add(const Duration(seconds: 10));
  while (DateTime.now().isBefore(deadline)) {
    if (await indexFile.exists()) {
      final rawIndex = await indexFile.readAsString();
      try {
        final decoded = jsonDecode(rawIndex);
        if (decoded is Map<String, dynamic>) {
          final extensions = decoded['extensions'];
          if (extensions is Map<String, dynamic> &&
              extensions.containsKey(extensionId)) {
            return;
          }
        }
      } on FormatException {
        // Zed may still be rewriting the index file; keep polling briefly.
      }
    }
    await Future<void>.delayed(const Duration(milliseconds: 500));
  }

  final lines = <String>[
    'The isolated Zed profile did not register the `$extensionId` extension.',
    'User data dir: ${userDataDir.path}',
    'Expected index: ${indexFile.path}',
  ];

  if (await logFile.exists()) {
    lines.add('Log file: ${logFile.path}');
    final relevantLogLines = extractExtensionStatus(
      await logFile.readAsString(),
    );
    if (relevantLogLines.isNotEmpty) {
      lines.add('Relevant log lines:');
      lines.addAll(relevantLogLines);
    }
    lines.add('cat "${logFile.path}"');
  }

  throw IntegrationFailure(lines.join('\n'));
}

List<String> extractExtensionHostFailures(String log) {
  final failures = <String>[];
  for (final line in const LineSplitter().convert(log)) {
    final lower = line.toLowerCase();
    final mentionsExtension =
        lower.contains('extension') ||
        lower.contains('wasm') ||
        lower.contains('debug adapter') ||
        lower.contains('language server');
    final looksBad =
        lower.contains(' error ') ||
        lower.contains(' failed ') ||
        lower.contains('panic') ||
        lower.contains('unreachable code');

    if (mentionsExtension && looksBad) {
      failures.add(line);
    }
  }
  return failures;
}

List<String> extractExtensionStatus(String log) {
  final relevant = <String>[];
  for (final line in const LineSplitter().convert(log)) {
    final lower = line.toLowerCase();
    if (lower.contains('extension_host') ||
        lower.contains('extensions updated') ||
        lower.contains('rebuilt extension index') ||
        lower.contains('installing extension')) {
      relevant.add(line);
    }
  }
  return relevant;
}

Future<void> copyDirectory(Directory source, Directory destination) async {
  await destination.create(recursive: true);
  await for (final entity in source.list(recursive: true)) {
    final relativePath = entity.path.substring(source.path.length + 1);
    final targetPath = joinPath(destination.path, relativePath);
    if (entity is Directory) {
      await Directory(targetPath).create(recursive: true);
    } else if (entity is File) {
      await File(targetPath).parent.create(recursive: true);
      await entity.copy(targetPath);
    }
  }
}

Future<File> _buildExtensionWasm() async {
  final cached = _cachedExtensionWasm;
  if (cached != null && await cached.exists()) {
    return cached;
  }

  final wasm = File(
    joinPath(
      Directory.current.path,
      'target',
      extensionTarget,
      extensionMode,
      'zed_dart.wasm',
    ),
  );
  if (await _isExtensionWasmCurrent(wasm)) {
    _cachedExtensionWasm = wasm;
    stdout.writeln('Reusing existing extension WebAssembly.');
    return wasm;
  }

  await _ensureWasmTargetInstalled();
  final cargo = resolveCargoExecutable();
  stdout.writeln('Building extension WebAssembly...');
  final result = await runCommand(
    cargo,
    ['build', '--target', extensionTarget, '--release'],
    workingDirectory: Directory.current.path,
    allowFailure: true,
  );

  if (result.exitCode != 0) {
    throw IntegrationFailure(
      'Failed to build extension WebAssembly.\n'
      '${result.stderr}\n'
      'The most likely cause is that the `$extensionTarget` target is not installed.\n'
      'Expected remediation: `rustup target add $extensionTarget`.',
    );
  }

  if (!await wasm.exists()) {
    throw IntegrationFailure(
      'Expected built extension at `${wasm.path}` but it was not found.',
    );
  }

  _cachedExtensionWasm = wasm;
  return wasm;
}

Future<bool> _isExtensionWasmCurrent(File wasm) async {
  if (!await wasm.exists()) {
    return false;
  }

  final wasmModified = await wasm.lastModified();
  for (final input in _extensionBuildInputs()) {
    if (!await input.exists()) {
      continue;
    }
    final newestInput = await _latestModified(input);
    if (newestInput.isAfter(wasmModified)) {
      return false;
    }
  }

  return true;
}

List<FileSystemEntity> _extensionBuildInputs() {
  return [
    File('Cargo.toml'),
    File('Cargo.lock'),
    File('extension.toml'),
    Directory('src'),
    Directory('languages'),
    Directory('debug_adapter_schemas'),
  ];
}

Future<DateTime> _latestModified(FileSystemEntity entity) async {
  if (entity is File) {
    return entity.lastModified();
  }

  if (entity is Directory) {
    var latest = (await entity.stat()).modified;
    await for (final child in entity.list(recursive: true)) {
      final childModified = await child.stat().then((stat) => stat.modified);
      if (childModified.isAfter(latest)) {
        latest = childModified;
      }
    }
    return latest;
  }

  return entity.stat().then((stat) => stat.modified);
}

Future<void> _ensureWasmTargetInstalled() async {
  final rustup = resolveRustupExecutable();
  var result = await runCommand(rustup, [
    'target',
    'list',
    '--installed',
  ], allowFailure: true);

  if (result.exitCode != 0) {
    throw IntegrationFailure(
      'Failed to query installed Rust targets.\n'
      'stdout:\n${result.stdout}\n'
      'stderr:\n${result.stderr}\n'
      'Expected remediation: run `rustup target add $extensionTarget`.',
    );
  }

  final installedTargets = const LineSplitter()
      .convert(result.stdout)
      .map((line) => line.trim())
      .where((line) => line.isNotEmpty)
      .toSet();

  if (!installedTargets.contains(extensionTarget)) {
    stdout.writeln(
      'Rust target `$extensionTarget` is not installed. Installing it now...',
    );

    final installResult = await runCommandWithProgress(
      rustup,
      ['target', 'add', extensionTarget],
      progressMessage: 'Installing Rust target `$extensionTarget`',
      allowFailure: true,
    );

    if (installResult.exitCode != 0) {
      throw IntegrationFailure(
        'Failed to install Rust target `$extensionTarget`.\n'
        'stdout:\n${installResult.stdout}\n'
        'stderr:\n${installResult.stderr}',
      );
    }

    result = await runCommand(rustup, [
      'target',
      'list',
      '--installed',
    ], allowFailure: true);

    final refreshedTargets = const LineSplitter()
        .convert(result.stdout)
        .map((line) => line.trim())
        .where((line) => line.isNotEmpty)
        .toSet();

    if (!refreshedTargets.contains(extensionTarget)) {
      throw IntegrationFailure(
        'Rust target `$extensionTarget` still does not appear to be installed after `rustup target add`.',
      );
    }
  }
}

Future<void> _writeExtensionIndex(Directory userDataDir) async {
  final indexFile = File(
    joinPath(userDataDir.path, 'extensions', 'index.json'),
  );
  await indexFile.parent.create(recursive: true);

  final manifest = await _readExtensionManifest();
  final index = <String, Object?>{
    'extensions': {
      extensionId: {'manifest': manifest, 'dev': true},
    },
    'themes': <String, Object?>{},
    'icon_themes': <String, Object?>{},
    'languages': {
      'Dart': {
        'extension': extensionId,
        'path': 'languages${Platform.pathSeparator}dart',
        'matcher': {
          'path_suffixes': ['dart'],
          'first_line_pattern': null,
          'modeline_aliases': <Object?>[],
        },
        'hidden': false,
        'grammar': 'dart',
      },
    },
  };

  await indexFile.writeAsString(
    const JsonEncoder.withIndent('  ').convert(index),
  );
}

Future<Map<String, Object?>> _readExtensionManifest() async {
  final content = await File('extension.toml').readAsLines();
  final manifest = <String, Object?>{
    'themes': <Object?>[],
    'icon_themes': <Object?>[],
    'context_servers': <String, Object?>{},
    'agent_servers': <String, Object?>{},
    'snippets': null,
    'capabilities': <Object?>[],
  };

  final authors = <String>[];
  var inAuthors = false;

  for (final rawLine in content) {
    final line = rawLine.trim();
    if (line.isEmpty || line.startsWith('#')) {
      continue;
    }

    if (inAuthors) {
      if (line == ']') {
        manifest['authors'] = authors;
        inAuthors = false;
        continue;
      }
      final value = _stripTomlString(line.replaceAll(',', ''));
      if (value != null) {
        authors.add(value);
      }
      continue;
    }

    if (line.startsWith('authors')) {
      inAuthors = true;
      continue;
    }

    if (line.startsWith('id = ')) {
      manifest['id'] = _stripTomlString(line.substring(5));
    } else if (line.startsWith('name = ')) {
      manifest['name'] = _stripTomlString(line.substring(7));
    } else if (line.startsWith('description = ')) {
      manifest['description'] = _stripTomlString(line.substring(14));
    } else if (line.startsWith('version = ')) {
      manifest['version'] = _stripTomlString(line.substring(10));
    } else if (line.startsWith('schema_version = ')) {
      manifest['schema_version'] = int.tryParse(line.substring(17).trim());
    } else if (line.startsWith('repository = ')) {
      manifest['repository'] = _stripTomlString(line.substring(13));
    }
  }

  manifest['lib'] = {'kind': 'Rust', 'version': '0.7.0'};
  manifest['languages'] = ['languages/dart'];
  manifest['grammars'] = {
    'dart': {
      'repository': 'https://github.com/UserNobody14/tree-sitter-dart',
      'rev': '80e23c07b64494f7e21090bb3450223ef0b192f4',
      'path': null,
    },
  };
  manifest['language_servers'] = {
    'dart': {
      'name': 'Dart LSP',
      'language': 'Dart',
      'languages': <Object?>[],
      'language_ids': <String, Object?>{},
      'code_action_kinds': null,
    },
  };
  manifest['slash_commands'] = {
    'flutter-devices': {
      'description': 'List available Flutter debug devices',
      'requires_argument': false,
      'tooltip_text': 'List Flutter devices',
    },
    'flutter-devtools': {
      'description': 'Build a Flutter DevTools URL from a VM service URI',
      'requires_argument': true,
      'tooltip_text': 'Build DevTools URL',
    },
    'flutter-hot-reload': {
      'description':
          'Temporarily trigger Flutter hot reload through a shell-backed attach shim',
      'requires_argument': false,
      'tooltip_text': 'Trigger Flutter hot reload',
    },
    'flutter-hot-restart': {
      'description':
          'Temporarily trigger Flutter hot restart through a shell-backed attach shim',
      'requires_argument': false,
      'tooltip_text': 'Trigger Flutter hot restart',
    },
  };

  return manifest;
}

String? _stripTomlString(String input) {
  final trimmed = input.trim();
  if (trimmed.startsWith('"') && trimmed.endsWith('"') && trimmed.length >= 2) {
    return trimmed.substring(1, trimmed.length - 1);
  }
  return null;
}
