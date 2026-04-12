use zed_extension_api::process::Command as ProcessCommand;
use zed_extension_api::{
    serde_json, Result, SlashCommandOutput, SlashCommandOutputSection, Worktree,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FlutterDevice {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) platform: Option<String>,
    pub(crate) emulator: Option<bool>,
    pub(crate) category: Option<String>,
}

pub(crate) fn parse_flutter_devices_json(output: &str) -> Result<Vec<FlutterDevice>, String> {
    let value: serde_json::Value = serde_json::from_str(output)
        .map_err(|err| format!("Invalid flutter devices JSON: {err}"))?;
    let devices = value
        .as_array()
        .ok_or_else(|| "Expected flutter devices output to be a JSON array".to_string())?;

    let mut parsed = Vec::new();
    for device in devices {
        let id = device
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Flutter device is missing string field `id`".to_string())?;
        let name = device
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Flutter device is missing string field `name`".to_string())?;

        parsed.push(FlutterDevice {
            id: id.to_string(),
            name: name.to_string(),
            platform: device
                .get("platformType")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            emulator: device.get("emulator").and_then(|v| v.as_bool()),
            category: device
                .get("category")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
        });
    }

    Ok(parsed)
}

pub(crate) fn filter_flutter_devices(
    devices: Vec<FlutterDevice>,
    query: &str,
) -> Vec<FlutterDevice> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return devices;
    }

    devices
        .into_iter()
        .filter(|device| {
            device.id.to_lowercase().contains(&query)
                || device.name.to_lowercase().contains(&query)
                || device
                    .platform
                    .as_ref()
                    .map(|p| p.to_lowercase().contains(&query))
                    .unwrap_or(false)
                || device
                    .category
                    .as_ref()
                    .map(|c| c.to_lowercase().contains(&query))
                    .unwrap_or(false)
        })
        .collect()
}

pub(crate) fn format_flutter_devices_output(devices: &[FlutterDevice]) -> SlashCommandOutput {
    let text = if devices.is_empty() {
        "No Flutter devices found.".to_string()
    } else {
        devices
            .iter()
            .map(|device| {
                let mut parts = vec![format!("{} (`{}`)", device.name, device.id)];
                if let Some(platform) = &device.platform {
                    parts.push(platform.clone());
                }
                if let Some(category) = &device.category {
                    parts.push(category.clone());
                }
                if let Some(true) = device.emulator {
                    parts.push("emulator".to_string());
                }
                format!("- {}", parts.join(" | "))
            })
            .collect::<Vec<String>>()
            .join("\n")
    };

    SlashCommandOutput {
        sections: vec![SlashCommandOutputSection {
            range: (0..text.len()).into(),
            label: "Flutter Devices".to_string(),
        }],
        text,
    }
}

pub(crate) fn flutter_devices_command(worktree: &Worktree) -> Result<Vec<FlutterDevice>, String> {
    let flutter = worktree
        .which("flutter")
        .unwrap_or_else(|| "flutter".to_string());
    let output = ProcessCommand::new(flutter)
        .arg("devices")
        .arg("--machine")
        .envs(worktree.shell_env())
        .output()?;

    if output.status != Some(0) {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!(
            "Failed to list Flutter devices{}",
            if message.is_empty() {
                String::new()
            } else {
                format!(": {message}")
            }
        ));
    }

    parse_flutter_devices_json(&String::from_utf8_lossy(&output.stdout))
}
