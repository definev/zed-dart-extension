# Zed Dart

A [Dart](https://dart.dev/) extension for [Zed](https://zed.dev).

## Debugging

The extension includes Dart and Flutter debug adapter support for Zed.

Currently supported:

- Dart and Flutter launch configurations
- Flutter attach configurations using `vmServiceUri` or `vmServiceInfoFile`
- Automatic per-launch Flutter `vmServiceInfoFile` generation under `.zed/dart/vmservice-info`
- Flutter device targeting through `device_id` or explicit `toolArgs`
- Task-to-debug scenario conversion for common `flutter run` and `dart run` tasks
- Runnable/task support for straight Dart entrypoints that import `dart:` SDK libraries
- Flutter device discovery with the `/flutter-devices` slash command
- DevTools URL generation with the `/flutter-devtools` slash command

Example `.zed/debug.json`:

```json
[
  {
    "label": "Debug Flutter App",
    "adapter": "Dart",
    "type": "flutter",
    "request": "launch",
    "program": "lib/main.dart",
    "device_id": "chrome"
  },
  {
    "label": "Attach Flutter App",
    "adapter": "Dart",
    "type": "flutter",
    "request": "attach",
    "program": "lib/main.dart",
    "vmServiceUri": "ws://127.0.0.1:50300/ws"
  }
]
```

Useful slash commands:

- `/flutter` shows help and dispatches to the Flutter subcommands below
- `/flutter-devices` lists machine-discovered Flutter devices
- `/flutter-devtools <vm-service-uri>` builds a local DevTools URL for a running app
- `/flutter-hot-reload [vm-service-uri-or-info-file]` temporarily drives `flutter attach` through a shell shim to request a hot reload
- `/flutter-hot-restart [vm-service-uri-or-info-file]` temporarily drives `flutter attach` through a shell shim to request a hot restart

If no argument is provided for the hot reload or hot restart commands, the extension will use the most recent generated VM service info file from `.zed/dart/vmservice-info`.

These slash commands are implemented as Zed extension slash commands. Per Zed's extension model, they are exposed in slash-command contexts and are separate from Zed Agent-specific slash commands.

## MCP Server

The extension also exposes a bundled MCP context server named `flutter-tools`.

It provides MCP tools that mirror the Flutter slash-command functionality:

- `flutter_devices`
- `flutter_devtools_url`
- `flutter_vm_probe`
- `flutter_hot_reload`
- `flutter_hot_restart`

Implementation notes:

- The MCP server source lives under [tool/mcp_server.dart](/C:/Users/bsutt/git/zed-dart/tool/mcp_server.dart) and [tool/src/mcp](/C:/Users/bsutt/git/zed-dart/tool/src/mcp).
- On first launch, the extension compiles the bundled Dart MCP server into a cached executable under the system temp directory.
- The bundled MCP server uses only `dart:` libraries and relative imports, so compiling it does not require `pub get` or pub.dev access.

Current limitation:

- Hot reload and hot restart currently use a temporary shell-backed slash-command workaround. Proper debugger-native actions still depend on newer Zed extension API support than this repository currently targets.
- The rest of the extension and integration harness are intended to be cross-platform; the hot reload / hot restart workaround is the remaining platform-specific production path.

## Documentation

See:
- [Zed Dart Language Docs](https://zed.dev/docs/languages/dart)
- [Dart LSP Support Docs](https://github.com/dart-lang/sdk/blob/main/pkg/analysis_server/tool/lsp_spec/README.md)

## Development

Use Zed's supported dev-extension workflow:

1. Open the Extensions page in Zed.
2. Click `Install Dev Extension`.
3. Select the repo root: [zed-dart](/C:/Users/bsutt/git/zed-dart)
4. After the first successful install, use `Rebuild` for local iteration.

Prerequisites:

- `cargo` must be on your PATH
- `rustup` must be on your PATH
- the Rust target `wasm32-wasip1` must be installed

The integration harness in [tool/integration.dart](/C:/Users/bsutt/git/zed-dart/tool/integration.dart) is now aligned to that workflow:

- `ensure-fixtures` creates Dart and Flutter fixture workspaces under the system temp directory
- `extension-load` verifies the local Rust toolchain by building the extension WebAssembly and then points you to Zed's `Install Dev Extension` / `Rebuild` flow
- `verify-vmservice flutter` assumes you already have Zed open with the dev extension installed and waits for Flutter VM service info files after you start debugging
- Toolchain and Zed executable discovery in the integration harness now uses PATH plus common platform-specific install locations instead of Windows-only paths

See the [Developing Extensions](https://zed.dev/docs/extensions/developing-extensions) section of the Zed docs for the upstream workflow.
