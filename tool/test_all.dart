import 'dart:io';

Future<void> main() async {
  await _run('cargo', ['test']);
  await _run('cargo', ['test', '--manifest-path', 'grammars/dart/Cargo.toml']);
}

Future<void> _run(String executable, List<String> arguments) async {
  final command = '$executable ${arguments.join(' ')}';
  stdout.writeln('Running: $command');
  final process = await Process.start(
    executable,
    arguments,
    mode: ProcessStartMode.inheritStdio,
  );
  final exitCode = await process.exitCode;
  if (exitCode != 0) {
    stderr.writeln('Command failed with exit code $exitCode: $command');
    exit(exitCode);
  }
  stdout.writeln('Finished: $command');
}
