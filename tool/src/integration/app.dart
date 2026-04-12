import 'dart:convert';
import 'dart:io';

import 'common.dart';
import 'extension_host.dart';
import 'fixtures.dart';

Future<void> runIntegrationTool(List<String> args) async {
  try {
    if (args.contains('--help') || args.contains('-h')) {
      _printUsage();
      return;
    }

    if (args.isEmpty) {
      await _runAllStages();
      return;
    }

    final command = args.first;
    final options = args.skip(1).toList();

    switch (command) {
      case 'ensure-fixtures':
        await ensureFixtures();
        stdout.writeln('Generated fixtures under ${fixtureRootPath()}');
        break;
      case 'extension-load':
        await _runExtensionLoad();
        break;
      case 'verify-vmservice':
        await _verifyVmServiceArtifacts(options);
        break;
      default:
        stderr.writeln('Unknown command: $command');
        _printUsage();
        exitCode = 64;
    }
  } on ProcessException catch (error) {
    stderr.writeln(
      'Process failed: ${error.executable} ${error.arguments.join(' ')}',
    );
    stderr.writeln(error.message);
    exitCode = 1;
  } on IntegrationFailure catch (error) {
    stderr.writeln(error.message);
    exitCode = 1;
  } catch (error) {
    stderr.writeln(error.toString());
    exitCode = 1;
  }
}

void _printUsage() {
  stdout.writeln('''
Zed Dart integration tooling

Usage:
  dart tool/integration.dart
  dart tool/integration.dart ensure-fixtures
  dart tool/integration.dart extension-load
  dart tool/integration.dart verify-vmservice <dart|flutter> [--launch-zed]

Notes:
  - With no arguments, the tool runs the full guided sequence:
    ensure fixtures, one extension-load check, then VM service verification for
    Flutter.
  - Fixtures are generated outside the repository under `${Directory.systemTemp.path}`
    to avoid workspace-routing conflicts with an already-open Zed window.
  - extension-load validates the local Rust toolchain by building the extension
    WebAssembly and then prints the supported Zed workflow: use `Install Dev
    Extension` once from the repo root, then use `Rebuild` for iteration.
  - verify-vmservice prompts you to start a debug session in Zed and then waits
    for generated `.zed/dart/vmservice-info/*.json` files to appear.
    By default it assumes you are using an already-open Zed window with the dev
    extension installed. Pass `--launch-zed` to open a fresh Zed window first.
''');
}

Future<void> _runAllStages() async {
  stdout.writeln('Running full integration sequence.');
  await ensureFixtures();
  stdout.writeln('Fixtures are ready.');
  await prepareExtensionWasm();

  stdout.writeln("Running 'extension-load' test.");
  await _runExtensionLoad();

  stdout.writeln("Running 'verify-vmservice' test for flutter.");
  await _verifyVmServiceArtifacts(['flutter']);
}

Future<void> _runExtensionLoad() async {
  final wasmPath = await prepareExtensionWasm();
  stdout.writeln('Extension build check passed.');
  stdout.writeln('Built WebAssembly: ${wasmPath.path}');
  stdout.writeln('Next step in Zed:');
  stdout.writeln('  1. Open the Extensions page.');
  stdout.writeln(
    '  2. Use `Install Dev Extension` and select `${Directory.current.path}`.',
  );
  stdout.writeln('  3. After the first install, use `Rebuild` for iteration.');
}

Future<void> _verifyVmServiceArtifacts(List<String> options) async {
  final fixtureName = requireFixtureName(options);
  final launchZed = options.contains('--launch-zed');

  if (fixtureName == 'dart') {
    throw IntegrationFailure(
      'VM service verification is currently only supported for `flutter`.\n'
      'This extension auto-generates `vmServiceInfoFile` artifacts for Flutter launches, not plain Dart launches.',
    );
  }

  final fixtureDir = fixtureDirectory(fixtureName);
  final infoDir = Directory(
    joinPath(fixtureDir.path, '.zed', 'dart', 'vmservice-info'),
  );

  Directory? userDataDir;
  Process? zedProcess;
  if (launchZed) {
    await ensureFixtures();
    userDataDir = await createIsolatedUserDataDir('${fixtureName}_manual');
    stdout.writeln('Launching Zed for `$fixtureName`...');
    zedProcess = await launchZedForManualUse(userDataDir);
    await waitForZedProfileStartup(userDataDir);
    stdout.writeln('Launched Zed for `$fixtureName`.');
    stdout.writeln('User data dir: ${userDataDir.path}');
    stdout.writeln(
      'Log file: ${joinPath(userDataDir.path, 'logs', 'Zed.log')}',
    );
    stdout.writeln(
      'Install the dev extension from `${Directory.current.path}` in that window before running the debug configuration.',
    );
  } else {
    stdout.writeln(
      'Using an already-open Zed window for `$fixtureName` with the dev extension installed.',
    );
  }

  try {
    const debugLabel = 'Debug Flutter Fixture';
    if (launchZed) {
      stdout.writeln(
        'In the launched Zed window, open `${fixtureDir.path}`, start the `$debugLabel` debug configuration, then wait here.',
      );
    } else {
      stdout.writeln(
        'Open `${fixtureDir.path}` in Zed, start the `$debugLabel` debug configuration, then wait here.',
      );
    }
    stdout.writeln(
      'Polling `${infoDir.path}` for VM service info files for up to 120 seconds...',
    );

    final files = await waitForVmServiceInfoFiles(infoDir);

    final summaries = <String>[];
    for (final file in files) {
      final jsonValue = jsonDecode(await file.readAsString());
      if (jsonValue is! Map<String, dynamic>) {
        throw IntegrationFailure(
          '`${file.path}` did not contain a JSON object.',
        );
      }

      final uri =
          jsonValue['uri'] ??
          jsonValue['vmServiceUri'] ??
          jsonValue['wsUri'] ??
          jsonValue['ws_uri'];
      if (uri is! String || uri.trim().isEmpty) {
        throw IntegrationFailure(
          '`${file.path}` does not contain a supported VM service URI field.',
        );
      }

      summaries.add('${file.uri.pathSegments.last}: $uri');
    }

    stdout.writeln(
      'Validated ${files.length} VM service info file(s) for `$fixtureName`:',
    );
    for (final summary in summaries) {
      stdout.writeln('  $summary');
    }
  } finally {
    zedProcess?.kill();
  }
}
