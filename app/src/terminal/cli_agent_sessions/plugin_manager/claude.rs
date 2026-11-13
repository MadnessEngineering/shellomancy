use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::{env, fs, io};

use async_trait::async_trait;
use serde_json::Value;

use super::{
    CliAgentPluginManager, PluginInstallError, PluginInstructionStep, PluginInstructions,
    compare_versions, run_cli_command_logged,
};
use crate::terminal::model::session::LocalCommandExecutor;
use crate::terminal::shell::ShellType;

// Shellomancy: the notification plugin is our fork, shipped inside the app
// bundle (resources/bundled/claude-plugin) and installed from disk.
const PLUGIN_KEY: &str = "shellomancy@shellomancy";
const MARKETPLACE_NAME: &str = "shellomancy";
/// Location of the bundled marketplace, relative to the bundled resources dir.
const BUNDLED_MARKETPLACE_SUBDIR: &str = "bundled/claude-plugin";

// Keep in sync with resources/bundled/claude-plugin/.claude-plugin/marketplace.json.
// 3.0.0 continues past Warp's 2.x, so a Warp plugin still installed reads as
// outdated and the update flow replaces it.
const MINIMUM_PLUGIN_VERSION: &str = "3.0.0";

/// Warp's notification plugin, uninstalled when ours goes in so both don't fire.
const WARP_PLUGIN_KEY: &str = "warp@claude-code-warp";

// The Oz platform plugin is Warp-cloud only and still comes from Warp's marketplace.
const PLATFORM_PLUGIN_KEY: &str = "oz-harness-support@claude-code-warp";
const WARP_MARKETPLACE_REPO: &str = "warpdotdev/claude-code-warp";
const WARP_MARKETPLACE_NAME: &str = "claude-code-warp";

// Keep in sync with the oz-harness-support plugin version in warpdotdev/claude-code-warp.
const MINIMUM_PLATFORM_PLUGIN_VERSION: &str = "1.1.2";

pub(super) struct ClaudeCodePluginManager {
    executor: LocalCommandExecutor,
    path_env_var: Option<String>,
}

impl ClaudeCodePluginManager {
    pub(super) fn new(
        shell_path: Option<PathBuf>,
        shell_type: Option<ShellType>,
        path_env_var: Option<String>,
    ) -> Self {
        let shell_type = shell_type.unwrap_or(ShellType::Bash);
        Self {
            executor: LocalCommandExecutor::new(shell_path, shell_type),
            path_env_var,
        }
    }

    async fn run_logged(&self, args: &[&str], log: &mut String) -> Result<(), PluginInstallError> {
        let env_vars = self
            .path_env_var
            .as_deref()
            .map(|path| HashMap::from([("PATH".to_owned(), path.to_owned())]));
        run_cli_command_logged("claude", args, &self.executor, env_vars, log).await
    }

    /// Installs the bundled Shellomancy plugin, replacing Warp's if present.
    ///
    /// The marketplace is removed and re-added on every run so a newer app
    /// bundle's plugin replaces the copy Claude cached from the last one. We use
    /// `plugin install` (not `plugin update`) because `marketplace remove`
    /// unlinks the plugin, so `plugin update` would fail with "Plugin is not
    /// installed".
    async fn install_from_bundle(&self, log: &mut String) -> Result<(), PluginInstallError> {
        let marketplace_dir = bundled_marketplace_dir().map_err(|message| PluginInstallError {
            message,
            log: log.clone(),
        })?;
        let marketplace_arg = shell_quote(&marketplace_dir.to_string_lossy());

        let _ = self
            .run_logged(&["plugin", "uninstall", WARP_PLUGIN_KEY], log)
            .await;
        if let Ok(claude_dir) = claude_home_dir()
            && !check_platform_plugin_installed(&claude_dir)
        {
            let _ = self
                .run_logged(
                    &["plugin", "marketplace", "remove", WARP_MARKETPLACE_NAME],
                    log,
                )
                .await;
        }
        let _ = self
            .run_logged(&["plugin", "marketplace", "remove", MARKETPLACE_NAME], log)
            .await;
        self.run_logged(&["plugin", "marketplace", "add", &marketplace_arg], log)
            .await?;
        self.run_logged(&["plugin", "install", PLUGIN_KEY], log)
            .await
    }
}

#[async_trait]
impl CliAgentPluginManager for ClaudeCodePluginManager {
    fn minimum_plugin_version(&self) -> &'static str {
        MINIMUM_PLUGIN_VERSION
    }

    fn can_auto_install(&self) -> bool {
        true
    }

    fn is_installed(&self) -> bool {
        let Ok(claude_dir) = claude_home_dir() else {
            return false;
        };
        check_installed(&claude_dir)
    }

    fn is_platform_plugin_installed(&self) -> bool {
        let Ok(claude_dir) = claude_home_dir() else {
            return false;
        };
        check_platform_plugin_installed(&claude_dir)
    }

    fn platform_plugin_needs_update(&self) -> bool {
        let Ok(claude_dir) = claude_home_dir() else {
            return false;
        };
        match installed_platform_plugin_version(&claude_dir) {
            Some(v) => compare_versions(&v, MINIMUM_PLATFORM_PLUGIN_VERSION).is_lt(),
            // No version field means very old plugin.
            None => check_platform_plugin_installed(&claude_dir),
        }
    }

    fn has_local_marketplace_override(&self) -> bool {
        let Ok(claude_dir) = claude_home_dir() else {
            return false;
        };
        claude_code_marketplace_has_local_override(&claude_dir)
    }

    /// Runs `claude plugin` CLI commands via the session shell.
    async fn install(&self) -> Result<(), PluginInstallError> {
        let mut log = String::new();
        self.install_from_bundle(&mut log).await
    }

    async fn update(&self) -> Result<(), PluginInstallError> {
        let mut log = String::new();
        self.install_from_bundle(&mut log).await?;

        // Sanity check: verify the on-disk version actually changed.
        let still_outdated = claude_home_dir()
            .ok()
            .and_then(|dir| installed_version(&dir))
            .map(|v| compare_versions(&v, MINIMUM_PLUGIN_VERSION).is_lt())
            .unwrap_or(true);
        if still_outdated {
            log.push_str("Post-update version check: plugin is still outdated\n");
            return Err(PluginInstallError {
                message: "Plugin update did not take effect".to_owned(),
                log,
            });
        }
        Ok(())
    }

    fn install_success_message(&self) -> &'static str {
        madness_themes::t("shellomancy.plugin.claudeInstalled")
    }

    fn update_success_message(&self) -> &'static str {
        madness_themes::t("shellomancy.plugin.claudeUpdated")
    }

    fn install_instructions(&self) -> &'static PluginInstructions {
        &INSTALL_INSTRUCTIONS
    }

    fn update_instructions(&self) -> &'static PluginInstructions {
        &UPDATE_INSTRUCTIONS
    }

    fn needs_update(&self) -> bool {
        let Ok(claude_dir) = claude_home_dir() else {
            return false;
        };
        match installed_version(&claude_dir) {
            Some(v) => compare_versions(&v, MINIMUM_PLUGIN_VERSION).is_lt(),
            // No version field means very old plugin.
            None => check_installed(&claude_dir),
        }
    }

    async fn install_platform_plugin(&self) -> Result<(), PluginInstallError> {
        let mut log = String::new();
        self.run_logged(
            &["plugin", "marketplace", "add", WARP_MARKETPLACE_REPO],
            &mut log,
        )
        .await?;
        self.run_logged(&["plugin", "install", PLATFORM_PLUGIN_KEY], &mut log)
            .await?;
        Ok(())
    }

    async fn update_platform_plugin(&self) -> Result<(), PluginInstallError> {
        let mut log = String::new();
        self.run_logged(
            &["plugin", "marketplace", "add", WARP_MARKETPLACE_REPO],
            &mut log,
        )
        .await?;
        self.run_logged(&["plugin", "install", PLATFORM_PLUGIN_KEY], &mut log)
            .await?;

        let still_outdated = claude_home_dir()
            .ok()
            .and_then(|dir| installed_platform_plugin_version(&dir))
            .map(|v| compare_versions(&v, MINIMUM_PLATFORM_PLUGIN_VERSION).is_lt())
            .unwrap_or(true);
        if still_outdated {
            log.push_str("Post-update version check: platform plugin is still outdated\n");
            return Err(PluginInstallError {
                message: "Platform plugin update did not take effect".to_owned(),
                log,
            });
        }
        Ok(())
    }
}

/// `claude plugin marketplace add` command for the bundled marketplace, built
/// once because the instructions below need `'static` strings.
static MARKETPLACE_ADD_COMMAND: LazyLock<&'static str> = LazyLock::new(|| {
    let dir = bundled_marketplace_dir().unwrap_or_else(|_| {
        PathBuf::from("/Applications/Shellomancy.app/Contents/Resources")
            .join(BUNDLED_MARKETPLACE_SUBDIR)
    });
    let command = format!(
        "claude plugin marketplace add {}",
        shell_quote(&dir.to_string_lossy())
    );
    Box::leak(command.into_boxed_str())
});

static INSTALL_INSTRUCTIONS: LazyLock<PluginInstructions> = LazyLock::new(|| PluginInstructions {
    title: "Bind Shellomancy to Claude Code",
    subtitle: "Ensure that jq is installed on your machine. Then, run these commands. \
               The plugin ships inside Shellomancy; nothing is downloaded.",
    steps: Box::leak(Box::new([
        PluginInstructionStep {
            description: "Add Shellomancy's bundled plugin marketplace",
            command: *MARKETPLACE_ADD_COMMAND,
            executable: true,
            link: None,
        },
        PluginInstructionStep {
            description: "Install the Shellomancy plugin",
            command: "claude plugin install shellomancy@shellomancy",
            executable: true,
            link: None,
        },
    ])),
    post_install_notes: &["Restart Claude Code to activate the plugin."],
});

static UPDATE_INSTRUCTIONS: LazyLock<PluginInstructions> = LazyLock::new(|| PluginInstructions {
    title: "Rebind Shellomancy to Claude Code",
    subtitle: "Run the following commands. They retire Warp's plugin if it is still installed.",
    steps: Box::leak(Box::new([
        PluginInstructionStep {
            description: "Retire Warp's plugin (if present)",
            command: "claude plugin uninstall warp@claude-code-warp",
            executable: true,
            link: None,
        },
        PluginInstructionStep {
            description: "Remove the existing Shellomancy marketplace (if present)",
            command: "claude plugin marketplace remove shellomancy",
            executable: true,
            link: None,
        },
        PluginInstructionStep {
            description: "Re-add the bundled marketplace",
            command: *MARKETPLACE_ADD_COMMAND,
            executable: true,
            link: None,
        },
        PluginInstructionStep {
            description: "Install the latest plugin version",
            command: "claude plugin install shellomancy@shellomancy",
            executable: true,
            link: None,
        },
    ])),
    post_install_notes: &["Restart Claude Code to activate the update."],
});

/// The marketplace directory inside this app's bundled resources.
fn bundled_marketplace_dir() -> Result<PathBuf, String> {
    let dir = warp_core::paths::bundled_resources_dir()
        .ok_or_else(|| "Could not locate Shellomancy's bundled resources".to_owned())?
        .join(BUNDLED_MARKETPLACE_SUBDIR);
    if !dir
        .join(".claude-plugin")
        .join("marketplace.json")
        .is_file()
    {
        return Err(format!(
            "This Shellomancy build has no bundled Claude Code plugin at {}",
            dir.display()
        ));
    }
    Ok(dir)
}

/// Single-quotes `value` for a POSIX shell; the plugin CLI runs through a login shell.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn check_installed(claude_dir: &Path) -> bool {
    check_plugin_installed(claude_dir, PLUGIN_KEY)
}

fn check_platform_plugin_installed(claude_dir: &Path) -> bool {
    check_plugin_installed(claude_dir, PLATFORM_PLUGIN_KEY)
}

fn check_plugin_installed(claude_dir: &Path, plugin_key: &str) -> bool {
    let plugins_path = claude_dir.join("plugins").join("installed_plugins.json");
    let Ok(contents) = fs::read_to_string(plugins_path) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&contents) else {
        return false;
    };
    parsed
        .get("plugins")
        .and_then(|p| p.get(plugin_key))
        .and_then(|v| v.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false)
}

/// Reads the installed version string for the Warp plugin, if present.
fn installed_version(claude_dir: &Path) -> Option<String> {
    installed_plugin_version(claude_dir, PLUGIN_KEY)
}

/// Reads the installed version string for the Oz platform plugin, if present.
fn installed_platform_plugin_version(claude_dir: &Path) -> Option<String> {
    installed_plugin_version(claude_dir, PLATFORM_PLUGIN_KEY)
}

fn installed_plugin_version(claude_dir: &Path, plugin_key: &str) -> Option<String> {
    let plugins_path = claude_dir.join("plugins").join("installed_plugins.json");
    let contents = fs::read_to_string(plugins_path).ok()?;
    let parsed: Value = serde_json::from_str(&contents).ok()?;
    parsed
        .get("plugins")?
        .get(plugin_key)?
        .as_array()?
        .first()?
        .get("version")?
        .as_str()
        .map(|s| s.to_owned())
}

fn claude_code_marketplace_has_local_override(claude_dir: &Path) -> bool {
    let settings_path = claude_dir.join("settings.json");
    let Ok(contents) = fs::read_to_string(settings_path) else {
        return false;
    };
    let Ok(settings) = serde_json::from_str::<Value>(&contents) else {
        return false;
    };

    settings
        .get("extraKnownMarketplaces")
        .and_then(|marketplaces| marketplaces.get(WARP_MARKETPLACE_NAME))
        .map(marketplace_entry_has_local_path)
        .unwrap_or(false)
}

fn marketplace_entry_has_local_path(entry: &Value) -> bool {
    let Some(source) = entry.get("source") else {
        return false;
    };
    match source {
        Value::Object(source) => {
            let source_kind = source.get("source").and_then(Value::as_str);
            let path = source.get("path").and_then(Value::as_str);
            source_kind == Some("directory") && path.map(is_local_marketplace_path).unwrap_or(false)
        }
        Value::String(source) => is_local_marketplace_path(source),
        _ => false,
    }
}

fn is_local_marketplace_path(source: &str) -> bool {
    source.starts_with('/')
        || source.starts_with("~/")
        || source.starts_with("./")
        || source.starts_with("../")
        || source.starts_with("file://")
}

/// Resolves the dir the Claude CLI reads/writes its state from.
///
/// Honors `CLAUDE_CONFIG_DIR` (respected by the Claude CLI, and set by the Oz
/// worker to a per-task dir), falling back to `~/.claude`. Must match where
/// `claude plugin install` writes, else install/verify checks read the wrong dir.
fn claude_home_dir() -> io::Result<PathBuf> {
    if let Ok(dir) = env::var("CLAUDE_CONFIG_DIR")
        && !dir.is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    dirs::home_dir()
        .map(|home| home.join(".claude"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "could not determine home directory",
            )
        })
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
