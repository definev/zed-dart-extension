# Test Fixtures

This directory documents the generated fixture workspaces used by the
integration tooling in `tool/integration.dart`.

The fixtures are created on demand outside the repository under the system temp
directory, so the repository does not need to commit a full generated Dart or
Flutter application and Zed does not route them into an already-open repo
workspace.
