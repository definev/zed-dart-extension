import 'dart:convert';
import 'dart:io';

import 'commands.dart';
import 'common.dart';

String requireFixtureName(List<String> options) {
  if (options.isEmpty) {
    throw IntegrationFailure('Expected a fixture name: `dart` or `flutter`.');
  }
  final fixtureName = options.first;
  if (fixtureName != 'dart' && fixtureName != 'flutter') {
    throw IntegrationFailure(
      'Unsupported fixture `$fixtureName`. Expected `dart` or `flutter`.',
    );
  }
  return fixtureName;
}

Future<void> ensureFixtures() async {
  await _ensureDartFixture();
  await _ensureFlutterFixture();
}

Directory fixtureDirectory(String fixtureName) {
  return Directory(joinPath(fixtureRootPath(), '${fixtureName}_app'));
}

Future<List<File>> waitForVmServiceInfoFiles(Directory infoDir) async {
  const timeout = Duration(seconds: 120);
  const pollInterval = Duration(seconds: 2);
  final deadline = DateTime.now().add(timeout);

  while (DateTime.now().isBefore(deadline)) {
    if (await infoDir.exists()) {
      final files = await infoDir
          .list()
          .where((entity) => entity is File && entity.path.endsWith('.json'))
          .cast<File>()
          .toList();
      if (files.isNotEmpty) {
        return files;
      }
    }
    await Future<void>.delayed(pollInterval);
  }

  throw IntegrationFailure(
    'Timed out waiting for VM service info files in `${infoDir.path}`.\n'
    'Start a debug session in Zed first, then rerun this command if needed.',
  );
}

Future<void> _ensureDartFixture() async {
  final directory = fixtureDirectory('dart');
  if (!await File(joinPath(directory.path, 'pubspec.yaml')).exists()) {
    await directory.create(recursive: true);
    await runCommand(resolveDartExecutable(), [
      'create',
      '--force',
      '--template',
      'console-simple',
      '.',
    ], workingDirectory: directory.path);
  }

  await _writeDebugConfig(directory, [
    {
      'label': 'Debug Dart CLI Fixture',
      'adapter': 'Dart',
      'type': 'dart',
      'request': 'launch',
      'program': 'bin/${_packageNameFromDirectory(directory)}.dart',
    },
  ]);
}

Future<void> _ensureFlutterFixture() async {
  final directory = fixtureDirectory('flutter');
  if (!await File(joinPath(directory.path, 'pubspec.yaml')).exists()) {
    await directory.create(recursive: true);
    await runCommand(resolveFlutterExecutable(), [
      'create',
      '--platforms=web',
      '.',
    ], workingDirectory: directory.path);
  }

  await _writeDebugConfig(directory, [
    {
      'label': 'Debug Flutter Fixture',
      'adapter': 'Dart',
      'type': 'flutter',
      'request': 'launch',
      'program': 'lib/main.dart',
      'device_id': 'chrome',
    },
  ]);
}

Future<void> _writeDebugConfig(
  Directory directory,
  List<Map<String, Object?>> configs,
) async {
  final zedDir = Directory(joinPath(directory.path, '.zed'));
  await zedDir.create(recursive: true);
  final debugFile = File(joinPath(zedDir.path, 'debug.json'));
  await debugFile.writeAsString(
    const JsonEncoder.withIndent('  ').convert(configs),
  );
}

String _packageNameFromDirectory(Directory directory) {
  final name = directory.uri.pathSegments
      .where((segment) => segment.isNotEmpty)
      .last;
  return name.replaceAll('-', '_');
}
