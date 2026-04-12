mod adapter;
mod device;
mod flutter;
mod mcp;
mod slash;

use zed::lsp::CompletionKind;
use zed::settings::LspSettings;
use zed::{CodeLabel, CodeLabelSpan};
use zed_extension_api::serde_json::json;
use zed_extension_api::{
    self as zed, serde_json, DebugAdapterBinary, DebugConfig, DebugRequest, DebugScenario,
    DebugTaskDefinition, Result, SlashCommandArgumentCompletion, SlashCommandOutput,
    SlashCommandOutputSection, StartDebuggingRequestArgumentsRequest, TaskTemplate, Worktree,
};

use crate::adapter::{
    build_debug_adapter_binary_from_config_with_worktree, build_debug_scenario_from_task,
    build_launch_config_value, dap_request_kind_from_config, infer_debug_mode,
};
use crate::device::{
    filter_flutter_devices, flutter_devices_command, format_flutter_devices_output,
};
use crate::mcp::{
    context_server_command as build_context_server_command,
    context_server_configuration as build_context_server_configuration,
};
use crate::slash::{build_devtools_url, probe_vm_service, run_flutter_attach_action};

struct DartBinary {
    pub path: String,
    pub args: Option<Vec<String>>,
}

struct DartExtension;

fn flutter_slash_command_help() -> SlashCommandOutput {
    let text = [
        "Available Flutter slash commands:",
        "- `/flutter devices` or `/flutter-devices`",
        "- `/flutter devtools <vm-service-uri>` or `/flutter-devtools <vm-service-uri>`",
        "- `/flutter hot-reload [vm-service-uri-or-info-file]` or `/flutter-hot-reload [...]`",
        "- `/flutter hot-restart [vm-service-uri-or-info-file]` or `/flutter-hot-restart [...]`",
        "- `/flutter vm-probe <vm-service-uri>` or `/flutter-vm-probe <vm-service-uri>`",
        "",
        "Note: these are Zed slash commands exposed by the extension. They are separate from Zed Agent-specific commands.",
    ]
    .join("\n");

    SlashCommandOutput {
        sections: vec![SlashCommandOutputSection {
            range: (0..text.len()).into(),
            label: "Flutter Commands".to_string(),
        }],
        text,
    }
}

impl DartExtension {
    fn language_server_binary(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<DartBinary> {
        let binary_settings = LspSettings::for_worktree("dart", worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.binary);
        let binary_args = binary_settings
            .as_ref()
            .and_then(|binary_settings| binary_settings.arguments.clone());

        if let Some(path) = binary_settings.and_then(|binary_settings| binary_settings.path) {
            return Ok(DartBinary {
                path,
                args: binary_args,
            });
        }

        if let Some(path) = worktree.which("dart") {
            return Ok(DartBinary {
                path,
                args: binary_args,
            });
        }

        Err(
            "dart must be installed from dart.dev/get-dart or pointed to by the LSP binary settings"
                .to_string(),
        )
    }
}

impl zed::Extension for DartExtension {
    fn new() -> Self {
        Self
    }

    fn complete_slash_command_argument(
        &self,
        command: zed::SlashCommand,
        args: Vec<String>,
    ) -> Result<Vec<SlashCommandArgumentCompletion>, String> {
        if command.name == "flutter" {
            let query = args.join(" ").to_lowercase();
            let suggestions = [
                "devices",
                "devtools http://127.0.0.1:12345/",
                "hot-reload",
                "hot-restart",
                "vm-probe ws://127.0.0.1:12345/ws",
            ];

            return Ok(suggestions
                .into_iter()
                .filter(|suggestion| suggestion.contains(&query))
                .map(|suggestion| SlashCommandArgumentCompletion {
                    label: suggestion.to_string(),
                    new_text: suggestion.to_string(),
                    run_command: true,
                })
                .collect());
        }

        if command.name == "flutter-devices" {
            let query = args.join(" ").to_lowercase();
            let suggestions = ["chrome", "macos", "windows", "linux", "android", "ios"];

            return Ok(suggestions
                .into_iter()
                .filter(|suggestion| suggestion.contains(&query))
                .map(|suggestion| SlashCommandArgumentCompletion {
                    label: suggestion.to_string(),
                    new_text: suggestion.to_string(),
                    run_command: true,
                })
                .collect());
        }

        if command.name == "flutter-devtools"
            || command.name == "flutter-hot-reload"
            || command.name == "flutter-hot-restart"
            || command.name == "flutter-vm-probe"
        {
            let query = args.join(" ").to_lowercase();
            let suggestions = [
                "http://127.0.0.1:12345/",
                "ws://127.0.0.1:12345/ws",
                "127.0.0.1:12345",
            ];

            return Ok(suggestions
                .into_iter()
                .filter(|suggestion| suggestion.contains(&query))
                .map(|suggestion| SlashCommandArgumentCompletion {
                    label: suggestion.to_string(),
                    new_text: suggestion.to_string(),
                    run_command: true,
                })
                .collect());
        }

        Ok(Vec::new())
    }

    fn run_slash_command(
        &self,
        command: zed::SlashCommand,
        args: Vec<String>,
        worktree: Option<&Worktree>,
    ) -> Result<SlashCommandOutput, String> {
        if command.name == "flutter" {
            let Some((subcommand, rest)) = args.split_first() else {
                return Ok(flutter_slash_command_help());
            };

            let forwarded = zed::SlashCommand {
                name: match subcommand.as_str() {
                    "devices" => "flutter-devices".to_string(),
                    "devtools" => "flutter-devtools".to_string(),
                    "hot-reload" => "flutter-hot-reload".to_string(),
                    "hot-restart" => "flutter-hot-restart".to_string(),
                    "vm-probe" => "flutter-vm-probe".to_string(),
                    _ => {
                        return Err(format!(
                            "Unknown Flutter subcommand `{subcommand}`. Run `/flutter` to see the supported commands."
                        ))
                    }
                },
                description: command.description,
                requires_argument: command.requires_argument,
                tooltip_text: command.tooltip_text,
            };

            return self.run_slash_command(forwarded, rest.to_vec(), worktree);
        }

        if command.name == "flutter-devices" {
            let worktree = worktree.ok_or_else(|| "Worktree is required".to_string())?;
            let query = args.join(" ");
            let devices = filter_flutter_devices(flutter_devices_command(worktree)?, &query);
            return Ok(format_flutter_devices_output(&devices));
        }

        if command.name == "flutter-devtools" {
            let vm_service_uri = args.join(" ");
            let url = build_devtools_url(&vm_service_uri)?;

            return Ok(SlashCommandOutput {
                sections: vec![SlashCommandOutputSection {
                    range: (0..url.len()).into(),
                    label: "DevTools URL".to_string(),
                }],
                text: url,
            });
        }

        if command.name == "flutter-hot-reload" {
            let worktree = worktree.ok_or_else(|| "Worktree is required".to_string())?;
            let vm_service_uri = args.join(" ");
            return run_flutter_attach_action(worktree, &vm_service_uri, 'r', "hot reload");
        }

        if command.name == "flutter-hot-restart" {
            let worktree = worktree.ok_or_else(|| "Worktree is required".to_string())?;
            let vm_service_uri = args.join(" ");
            return run_flutter_attach_action(worktree, &vm_service_uri, 'R', "hot restart");
        }

        if command.name == "flutter-vm-probe" {
            let vm_service_uri = args.join(" ");
            return probe_vm_service(&vm_service_uri);
        }

        Err("Invalid slash command.".to_string())
    }

    fn context_server_command(
        &mut self,
        context_server_id: &zed::ContextServerId,
        project: &zed::Project,
    ) -> Result<zed::Command> {
        build_context_server_command(context_server_id, project)
    }

    fn context_server_configuration(
        &mut self,
        context_server_id: &zed::ContextServerId,
        project: &zed::Project,
    ) -> Result<Option<zed::ContextServerConfiguration>> {
        build_context_server_configuration(context_server_id, project)
    }

    fn get_dap_binary(
        &mut self,
        _adapter_name: String,
        config: DebugTaskDefinition,
        _user_provided_debug_adapter_path: Option<String>,
        worktree: &Worktree,
    ) -> Result<DebugAdapterBinary, String> {
        let user_config: serde_json::Value = serde_json::from_str(&config.config)
            .map_err(|e| format!("Failed to parse debug config: {e}"))?;

        build_debug_adapter_binary_from_config_with_worktree(
            &user_config,
            &worktree.root_path(),
            worktree,
        )
    }

    fn dap_request_kind(
        &mut self,
        _adapter_name: String,
        config: serde_json::Value,
    ) -> Result<StartDebuggingRequestArgumentsRequest, String> {
        dap_request_kind_from_config(&config)
    }

    fn dap_config_to_scenario(&mut self, config: DebugConfig) -> Result<DebugScenario, String> {
        let config_json = match config.request {
            DebugRequest::Launch(launch) => build_launch_config_value(
                infer_debug_mode(&launch.program, launch.cwd.as_deref()),
                &launch.program,
                launch.cwd,
                launch.args,
                launch.envs,
                config.stop_on_entry.unwrap_or(false),
                Vec::new(),
                None,
            )
            .to_string(),
            DebugRequest::Attach(_) => json!({
                "type": "dart",
                "request": "attach",
                "program": "lib/main.dart",
                "stopOnEntry": config.stop_on_entry.unwrap_or(false),
                "sendLogsToClient": true
            })
            .to_string(),
        };

        Ok(DebugScenario {
            adapter: config.adapter,
            label: config.label,
            build: None,
            config: config_json,
            tcp_connection: None,
        })
    }

    fn dap_locator_create_scenario(
        &mut self,
        locator_name: String,
        build_task: TaskTemplate,
        resolved_label: String,
        debug_adapter_name: String,
    ) -> Option<DebugScenario> {
        if locator_name != "dart" || debug_adapter_name != "Dart" {
            return None;
        }

        build_debug_scenario_from_task(&build_task, resolved_label, debug_adapter_name)
    }

    fn run_dap_locator(
        &mut self,
        _locator_name: String,
        _build_task: TaskTemplate,
    ) -> Result<DebugRequest, String> {
        Err("Dart locator scenarios resolve directly and do not use a build phase".to_string())
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let dart_binary = self.language_server_binary(language_server_id, worktree)?;

        Ok(zed::Command {
            command: dart_binary.path,
            args: dart_binary.args.unwrap_or_else(|| {
                vec!["language-server".to_string(), "--protocol=lsp".to_string()]
            }),
            env: Default::default(),
        })
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        let settings = LspSettings::for_worktree("dart", worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.settings.clone())
            .unwrap_or_default();

        Ok(Some(serde_json::json!({
            "dart": settings
        })))
    }

    fn label_for_completion(
        &self,
        _language_server_id: &zed::LanguageServerId,
        completion: zed::lsp::Completion,
    ) -> Option<CodeLabel> {
        let arrow = " â†’ ";

        match completion.kind? {
            CompletionKind::Class => Some(CodeLabel {
                filter_range: (0..completion.label.len()).into(),
                spans: vec![CodeLabelSpan::literal(
                    completion.label,
                    Some("type".into()),
                )],
                code: String::new(),
            }),
            CompletionKind::Function | CompletionKind::Constructor | CompletionKind::Method => {
                let mut parts = completion.detail.as_ref()?.split(arrow);
                let (name, _) = completion.label.split_once('(')?;
                let parameter_list = parts.next()?;
                let return_type = parts.next()?;
                let fn_name = " a";
                let fat_arrow = " => ";
                let call_expr = "();";

                let code =
                    format!("{return_type}{fn_name}{parameter_list}{fat_arrow}{name}{call_expr}");

                let parameter_list_start = return_type.len() + fn_name.len();

                Some(CodeLabel {
                    spans: vec![
                        CodeLabelSpan::code_range(
                            code.len() - call_expr.len() - name.len()..code.len() - call_expr.len(),
                        ),
                        CodeLabelSpan::code_range(
                            parameter_list_start..parameter_list_start + parameter_list.len(),
                        ),
                        CodeLabelSpan::literal(arrow, None),
                        CodeLabelSpan::code_range(0..return_type.len()),
                    ],
                    filter_range: (0..name.len()).into(),
                    code,
                })
            }
            CompletionKind::Property => {
                let class_start = "class A {";
                let get = " get ";
                let property_end = " => a; }";
                let ty = completion.detail?;
                let name = completion.label;

                let code = format!("{class_start}{ty}{get}{name}{property_end}");
                let name_start = class_start.len() + ty.len() + get.len();

                Some(CodeLabel {
                    spans: vec![
                        CodeLabelSpan::code_range(name_start..name_start + name.len()),
                        CodeLabelSpan::literal(arrow, None),
                        CodeLabelSpan::code_range(class_start.len()..class_start.len() + ty.len()),
                    ],
                    filter_range: (0..name.len()).into(),
                    code,
                })
            }
            CompletionKind::Variable => {
                let name = completion.label;

                Some(CodeLabel {
                    filter_range: (0..name.len()).into(),
                    spans: vec![CodeLabelSpan::literal(name, Some("variable".into()))],
                    code: String::new(),
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;

zed::register_extension!(DartExtension);
