import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'common.dart';

Future<CommandResult> runCommand(
  String executable,
  List<String> arguments, {
  String? workingDirectory,
  bool allowFailure = false,
}) async {
  final result = await Process.run(
    executable,
    arguments,
    workingDirectory: workingDirectory,
    environment: {
      ...Platform.environment,
      'CI': 'true',
      'DART_DISABLE_ANALYTICS': 'true',
      'FLUTTER_SUPPRESS_ANALYTICS': 'true',
    },
  );
  if (!allowFailure && result.exitCode != 0) {
    throw IntegrationFailure(
      'Command failed: $executable ${arguments.join(' ')}\n'
      'stdout:\n${result.stdout}\n'
      'stderr:\n${result.stderr}',
    );
  }
  return CommandResult(
    exitCode: result.exitCode,
    stdout: '${result.stdout}',
    stderr: '${result.stderr}',
  );
}

Future<CommandResult> runCommandWithProgress(
  String executable,
  List<String> arguments, {
  required String progressMessage,
  String? workingDirectory,
  bool allowFailure = false,
}) async {
  final process = await Process.start(
    executable,
    arguments,
    workingDirectory: workingDirectory,
    environment: {
      ...Platform.environment,
      'CI': 'true',
      'DART_DISABLE_ANALYTICS': 'true',
      'FLUTTER_SUPPRESS_ANALYTICS': 'true',
    },
  );

  final stdoutBuffer = StringBuffer();
  final stderrBuffer = StringBuffer();
  final stdoutSub = process.stdout
      .transform(utf8.decoder)
      .listen(stdoutBuffer.write);
  final stderrSub = process.stderr
      .transform(utf8.decoder)
      .listen(stderrBuffer.write);

  final frames = ['|', '/', '-', '\\'];
  var frameIndex = 0;
  stdout.write('$progressMessage ${frames[frameIndex]}');
  final timer = Timer.periodic(const Duration(milliseconds: 150), (_) {
    frameIndex = (frameIndex + 1) % frames.length;
    stdout.write('\r$progressMessage ${frames[frameIndex]}');
  });

  final exitCode = await process.exitCode;
  await stdoutSub.cancel();
  await stderrSub.cancel();
  timer.cancel();
  stdout.write('\r$progressMessage ... done\n');

  final result = CommandResult(
    exitCode: exitCode,
    stdout: stdoutBuffer.toString(),
    stderr: stderrBuffer.toString(),
  );

  if (!allowFailure && result.exitCode != 0) {
    throw IntegrationFailure(
      'Command failed: $executable ${arguments.join(' ')}\n'
      'stdout:\n${result.stdout}\n'
      'stderr:\n${result.stderr}',
    );
  }

  return result;
}

String resolveZedExecutable() {
  return resolveExecutable(
    logicalName: 'Zed',
    pathEntries: const ['Zed', 'zed', 'Zed.exe', 'zed.exe'],
    extraCandidates: switch (currentHostOs) {
      HostOs.windows => [
        _envPath('LOCALAPPDATA', 'Programs', 'Zed', 'Zed.exe'),
        _envPath5('LOCALAPPDATA', 'Programs', 'Zed', 'bin', 'Zed.exe'),
      ],
      HostOs.macos => [
        '/Applications/Zed.app/Contents/MacOS/Zed',
        '/Applications/Zed Preview.app/Contents/MacOS/Zed Preview',
      ],
      HostOs.linux => [
        '/usr/bin/zed',
        '/usr/local/bin/zed',
        '/var/lib/flatpak/exports/bin/dev.zed.Zed',
      ],
    },
  );
}

String resolveCargoExecutable() {
  return resolveExecutable(
    logicalName: 'cargo',
    pathEntries: executableBasenames('cargo'),
    extraCandidates: _cargoBinCandidates('cargo'),
  );
}

String resolveRustupExecutable() {
  return resolveExecutable(
    logicalName: 'rustup',
    pathEntries: executableBasenames('rustup'),
    extraCandidates: _cargoBinCandidates('rustup'),
  );
}

String resolveDartExecutable() {
  final dart = resolveExecutable(
    logicalName: 'dart',
    pathEntries: executableBasenames('dart'),
    extraCandidates: const [],
    includeResolvedExecutable: true,
    shouldUseCandidate: (candidate) {
      final normalized = candidate.path.replaceAll('\\', '/').toLowerCase();
      return !normalized.contains('/.dswitch/active/');
    },
  );

  return dart;
}

String resolveFlutterExecutable() {
  return resolveExecutable(
    logicalName: 'flutter',
    pathEntries: executableBasenames('flutter'),
    extraCandidates: const [],
  );
}

enum HostOs { windows, macos, linux }

HostOs get currentHostOs {
  if (Platform.isWindows) {
    return HostOs.windows;
  }
  if (Platform.isMacOS) {
    return HostOs.macos;
  }
  return HostOs.linux;
}

List<String> executableBasenames(String name) {
  if (!Platform.isWindows) {
    return [name];
  }
  return ['$name.bat', '$name.exe', name];
}

List<String> _cargoBinCandidates(String executable) {
  final home = homeDirectoryPath;
  if (home == null) {
    return const [];
  }
  return executableBasenames(
    executable,
  ).map((name) => joinPath(home, '.cargo', 'bin', name)).toList();
}

String? get homeDirectoryPath {
  final home = Platform.environment['HOME'];
  if (home != null && home.isNotEmpty) {
    return home;
  }

  final userProfile = Platform.environment['USERPROFILE'];
  if (userProfile != null && userProfile.isNotEmpty) {
    return userProfile;
  }

  return null;
}

String? _envPath(String variable, String second, String third, String fourth) {
  final root = Platform.environment[variable];
  if (root == null || root.isEmpty) {
    return null;
  }
  return joinPath(root, second, third, fourth);
}

String? _envPath5(
  String variable,
  String second,
  String third,
  String fourth,
  String fifth,
) {
  final root = Platform.environment[variable];
  if (root == null || root.isEmpty) {
    return null;
  }
  return joinPath(root, second, third, fourth, fifth);
}

List<String> _pathEntries() {
  final path = Platform.environment['PATH'];
  if (path == null || path.isEmpty) {
    return const [];
  }
  return path.split(Platform.isWindows ? ';' : ':');
}

typedef CandidateFilter = bool Function(File candidate);

String resolveExecutable({
  required String logicalName,
  required List<String> pathEntries,
  required List<String?> extraCandidates,
  bool includeResolvedExecutable = false,
  CandidateFilter? shouldUseCandidate,
}) {
  final isUsable = shouldUseCandidate ?? ((_) => true);

  for (final entry in _pathEntries()) {
    if (entry.trim().isEmpty) {
      continue;
    }
    for (final executable in pathEntries) {
      final candidate = File(joinPath(entry.trim(), executable));
      if (candidate.existsSync() && isUsable(candidate)) {
        return candidate.path;
      }
    }
  }

  for (final candidatePath in extraCandidates.whereType<String>()) {
    final candidate = File(candidatePath);
    if (candidate.existsSync() && isUsable(candidate)) {
      return candidate.path;
    }
  }

  if (includeResolvedExecutable) {
    final candidate = File(Platform.resolvedExecutable);
    if (candidate.existsSync() && isUsable(candidate)) {
      return candidate.path;
    }
  }

  throw IntegrationFailure('Could not find $logicalName executable.');
}
