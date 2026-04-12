import 'dart:io';

const integrationRoot = '.zed-integration';
const extensionTarget = 'wasm32-wasip1';
const extensionMode = 'release';
const extensionId = 'dart';

String joinPath(
  String first, [
  String? second,
  String? third,
  String? fourth,
  String? fifth,
  String? sixth,
]) {
  final parts = [
    first,
    second,
    third,
    fourth,
    fifth,
    sixth,
  ].whereType<String>().toList();
  return parts.join(Platform.pathSeparator);
}

String fixtureRootPath() {
  final temp = Directory.systemTemp.path;
  return joinPath(temp, 'zed-dart-fixtures');
}

Future<Directory> createIsolatedUserDataDir(String fixtureName) async {
  final root = Directory(
    joinPath(
      Directory.current.path,
      integrationRoot,
      '${fixtureName}_${DateTime.now().millisecondsSinceEpoch}',
    ),
  );
  await root.create(recursive: true);
  return root;
}

final class CommandResult {
  CommandResult({
    required this.exitCode,
    required this.stdout,
    required this.stderr,
  });

  final int exitCode;
  final String stdout;
  final String stderr;
}

final class IntegrationFailure implements Exception {
  IntegrationFailure(this.message);

  final String message;

  @override
  String toString() => message;
}
