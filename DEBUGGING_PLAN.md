# Dart/Flutter Debugging Plan for Zed

## Goal

Bring this extension to a point where Dart and Flutter applications can be debugged reliably in Zed, including practical access to Flutter DevTools.

## Delivery Principle

Testing is a first-class requirement for this work.

- Every phase must include unit and integration coverage appropriate to the change.
- No debugger behavior change should merge without regression coverage for the affected config or workflow.
- Testing is not a final polish step. It is part of the definition of done for each phase.

## Current State

- The extension already starts the Dart/Flutter DAP via `dart debug_adapter` / `flutter debug_adapter`.
- Debugging support is implemented only through `get_dap_binary` and `dap_request_kind`.
- The extension does not implement Zed's newer debug scenario or debug locator APIs.
- DevTools integration is not implemented.

## Current Gaps

### Baseline debugger issues

- Windows debug config uses the tool binary name as the DAP `type`, which is incorrect for `flutter.bat` / `dart.bat`.
- `stopOnEntry` is ignored and forced to `false`.
- `flutterMode` is hardcoded to `debug`.
- `vmServiceUri` handling is incomplete for attach flows.
- `device_id` is not translated into Flutter tool arguments like `-d <device>`.
- FVM tasks are malformed because they use `command = "fvm flutter"` instead of `command = "fvm"` plus args.

### Workflow gaps

- No debug locators are registered, so tasks are not converted into debug sessions automatically.
- No scenario generation exists for Zed's newer debugger UI.
- No hot reload or hot restart support in the extension itself.
- No DevTools launch or URL surfacing support.

### Project gaps

- No automated test coverage for debug config generation or runnable-to-debug resolution.
- Local verification in this workspace is limited because `cargo` is not installed here.

## Upstream Status

### Relevant upstream items

- `zed-extensions/dart#64`: attach debugger failures
- `zed-extensions/dart#45`: rerun session issues
- `zed-extensions/dart#50`: Flutter debugger maturity concerns
- `zed-extensions/dart#66`: baseline debug config fixes
- `zed-extensions/dart#72`: hot reload and hot restart custom actions
- `zed-industries/zed#51873`: Zed-side custom debug actions API

## Issue Inputs

The roadmap should be driven by the current open issues that affect debugger usefulness in real projects.

### Major milestone issue set

These issues define the first major debugger milestone for this fork:

- `zed-extensions/dart#64`: attach debugger failures
- `zed-extensions/dart#45`: rerun session instability
- `zed-extensions/dart#50`: Flutter debugger maturity and missing practical workflows
- `zed-extensions/dart#33`: nested-module and non-root `pubspec.yaml` workflows
- `zed-extensions/dart#63`: hot reload on save

### How these issues affect scope

- `#64` means attach must be treated as a first-class workflow, not an optional follow-up.
- `#45` means rerun-session stability needs explicit regression testing.
- `#50` confirms that current debugging is incomplete even when basic breakpoint support works.
- `#33` means `cwd`, nested modules, and project-root resolution must be covered early in integration testing.
- `#63` confirms that save-triggered hot reload is an expected Flutter workflow.

### Additional objective

- Add VS Code-style interactive Flutter device selection rather than relying only on manual `device_id` configuration.

### Adjacent issues that should influence design

- `zed-extensions/dart#77`: LSP initialization failures
- `zed-extensions/dart#53`: inconsistent run indicators

These are not the primary debugger deliverables, but they affect the perceived quality and reliability of the debugging experience.

### Issues intentionally out of scope for this milestone

The following should not drive the debugger implementation directly:

- completion behavior issues
- syntax highlighting issues
- outline and breadcrumb improvements
- dartdoc support
- closing labels
- test/implementation-file navigation
- line-length default handling

### Assessment of `#66`

- `#66` is a good base patch and still appears applicable.
- It fixes the most obvious correctness problems in:
  - `src/dart.rs`
  - `debug_adapter_schemas/Dart.json`
  - `languages/dart/tasks.json`
- It should not be treated as the final solution because it still does not fully address device selection, locators, or DevTools.

## Major Milestone

### Milestone 1: Reliable Dart/Flutter Debugging

The first major milestone for this fork is:

- stable launch support
- stable attach support
- stable rerun-session behavior
- working nested-module and non-root `pubspec.yaml` workflows
- explicit and reliable device targeting, with a path to interactive device selection
- a credible path toward hot reload support

This milestone is considered complete only when the issue set above is addressed well enough that Dart and Flutter debugging is usable in normal day-to-day development, not just in ideal demo setups.

### Milestone 1 quality gates

- Dart launch works in standard projects.
- Flutter launch works with explicit device targeting.
- Device selection is reliable for configured launches, and the roadmap includes interactive device selection as the next usability step.
- Attach works against a running Flutter app.
- Rerun works repeatedly within the same session.
- Nested-module projects work with correct `cwd` and root resolution.
- Integration fixtures cover at least one Dart project and one Flutter project.
- Regression tests exist for launch, attach, rerun, and nested-module handling.

## Implementation Plan

### Phase 1: Land baseline debug fixes

Apply the equivalent of upstream `#66` to this fork.

Scope:

- Fix Windows DAP `type` handling.
- Respect `stopOnEntry`.
- Make `flutterMode` configurable.
- Add `toolArgs`.
- Add `env`.
- Avoid sending `vmServiceUri` when absent.
- Enable adapter log forwarding to Zed.
- Fix FVM task definitions.

Testing requirements:

- Add unit tests for debug config serialization.
- Add unit tests for `dap_request_kind`.
- Add regression tests for:
  - Windows `type` handling
  - `stopOnEntry`
  - `flutterMode`
  - `toolArgs`
  - `env`
  - absent vs present `vmServiceUri`
  - FVM task shape

Exit criteria:

- Launch works on Windows, macOS, and Linux for basic Dart and Flutter configs.
- FVM-backed tasks invoke correctly.
- Core config generation behavior is covered by unit tests.

### Phase 2: Fix Flutter launch ergonomics

Improve how Flutter launches are constructed.

Scope:

- Convert `device_id` into effective Flutter tool args such as `-d <device>`.
- Support common Flutter launch variants through config:
  - flavors
  - web server targets
  - emulator and physical device IDs
  - profile and release modes where appropriate
- Normalize `cwd` behavior for multi-module workspaces.

Testing requirements:

- Add unit tests for device and tool argument generation.
- Add integration-style verification against at least one sample Flutter project.
- Add regression coverage for multi-module `cwd` resolution.

Exit criteria:

- Flutter launches target the requested device deterministically.
- Common real-world project layouts can be launched without manual command rewriting.
- Launch argument construction is covered by tests.

### Phase 3: Implement native Zed debug flows

Adopt Zed's scenario and locator APIs.

Scope:

- Register a debug locator in `extension.toml`.
- Implement:
  - `dap_config_to_scenario`
  - `dap_locator_create_scenario`
  - `run_dap_locator`
- Map runnable tasks into debug sessions for Dart and Flutter entrypoints.

Testing requirements:

- Add unit tests for scenario generation from generic debug configs.
- Add unit tests for locator acceptance and rejection behavior.
- Add integration-style coverage that exercises runnable-to-debug conversion for:
  - Dart entrypoints
  - Flutter entrypoints
  - test tasks where supported

Exit criteria:

- Users can start debug sessions from run tasks and newer debugger UI flows.
- Manual `.zed/debug.json` remains supported, but is no longer the only path.
- Scenario and locator logic are covered by tests.

### Phase 4: Stabilize attach and rerun

Target known debugger reliability problems.

Scope:

- Fix attach behavior around `vmServiceUri`.
- Verify accepted URI formats and normalize where necessary.
- Investigate rerun-session failures and ensure repeat launches behave consistently.
- Add explicit logging around adapter startup and attach resolution.

Testing requirements:

- Add unit tests for `vmServiceUri` normalization and attach config generation.
- Add integration-style verification against a running sample app for attach.
- Add regression coverage for rerun behavior where it can be simulated or validated through scripted workflows.

Exit criteria:

- Attach to a running Flutter app works reliably.
- Rerun session works more than once per project session.
- Attach and rerun changes are backed by regression coverage.

### Phase 5: Add interactive device selection

Add a first-class device-selection workflow for Flutter launches.

Scope:

- Discover devices using Flutter's machine-readable device listing.
- Surface selectable devices through the extension flow instead of requiring users to hand-edit `device_id`.
- Carry the selected device into generated debug scenarios and launch configs.
- Preserve manual `device_id` configuration as an override path.

Testing requirements:

- Add unit tests for device-list parsing and scenario generation using selected devices.
- Add integration-style verification with at least two device targets where feasible, such as `chrome` plus one desktop/mobile target.
- Add regression coverage ensuring manual `device_id` still overrides defaults when explicitly provided.

Exit criteria:

- Users can choose a Flutter device without editing raw config.
- Generated launch configs use the selected device consistently.
- Manual device configuration continues to work.

### Phase 6: Add hot reload and hot restart

Adopt extension-declared debug actions once supported by Zed.

Scope:

- Add custom debug actions for:
  - hot reload
  - hot restart
- Hook reload to save events where appropriate.
- Rebase any implementation on top of the baseline fixes from Phase 1.

Dependency:

- Zed support for extension custom debug actions, tracked in `zed-industries/zed#51873`.

Testing requirements:

- Add integration-style verification for toolbar-triggered reload and restart.
- Add regression coverage for save-triggered reload behavior.
- Validate behavior against a sample Flutter application where state preservation can be observed.

Exit criteria:

- Flutter sessions expose hot reload and hot restart in Zed.
- Save-triggered reload works when enabled.
- Hot reload and hot restart behavior are covered by automated checks where feasible and by documented integration verification.

### Phase 7: Add DevTools access

Provide a practical DevTools workflow.

Short-term target:

- Surface the DevTools URL in debug output.
- Add a command or action to open DevTools in the browser for the active VM service.

Medium-term target:

- If Zed gains extension webview or panel support suitable for this, evaluate embedded DevTools.

Notes:

- Browser-based DevTools launch is the pragmatic target today.
- Embedded DevTools likely depends on Zed platform capabilities beyond this extension.

Testing requirements:

- Add unit tests for DevTools URL construction from VM service data.
- Add integration-style verification that an active Flutter debug session exposes a usable DevTools launch path.

Exit criteria:

- A running Flutter debug session exposes a one-step path to open DevTools.
- DevTools launch behavior is covered by tests and scripted verification.

### Phase 8: Add verification coverage

Consolidate and expand the testing strategy so future debugger work remains stable.

Scope:

- Fill remaining unit-test gaps.
- Build reusable integration fixtures for Dart and Flutter sample apps.
- Add CI coverage for the extension test suite.
- Add manual verification docs for:
  - Dart CLI apps
  - Flutter mobile
  - Flutter web
  - launch
  - attach
  - rerun
  - hot reload
  - hot restart
  - DevTools launch

Exit criteria:

- Debugging changes can be validated without relying only on ad hoc manual testing.
- CI and local verification cover the most failure-prone debugger workflows.

## Recommended Order

1. Baseline fixes from `#66`
2. Flutter launch and device selection cleanup
3. Zed debug scenarios and locators
4. Attach and rerun stabilization
5. Interactive device selection
6. Hot reload and hot restart
7. DevTools launch support
8. Test coverage and polish

## Practical Recommendation

Start by implementing the equivalent of `#66` in this fork, but extend it so Flutter `device_id` becomes effective tool arguments. That gives a stable base for the larger debugging roadmap and avoids building DevTools support on top of a fragile launch layer.

## Quality Bar

- No phase is complete without the tests listed in that phase.
- Unit tests should cover config generation, scenario conversion, and normalization logic.
- Integration tests should cover launch, attach, rerun, reload, restart, and DevTools access as the features land.
- When full automation is not feasible, add scripted verification steps and sample fixtures immediately, then automate them as tooling allows.
