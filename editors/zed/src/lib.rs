//! Zed extension: starts `asm-check lsp` for Assembly files.

use zed_extension_api::settings::LspSettings;
use zed_extension_api::{self as zed, serde_json, LanguageServerId, Result};

struct AsmCheck;

impl zed::Extension for AsmCheck {
    fn new() -> Self {
        AsmCheck
    }

    /// Uses `lsp.asm-check.binary` from the settings when present, otherwise
    /// the `asm-check` found on `PATH`.
    fn language_server_command(
        &mut self,
        id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.binary);
        let command = binary
            .as_ref()
            .and_then(|binary| binary.path.clone())
            .or_else(|| worktree.which("asm-check"))
            .ok_or_else(|| {
                "`asm-check` was not found on PATH. Install it with \
                 `cargo install --path .` from the asm-check repository, or set \
                 `lsp.asm-check.binary.path` in the Zed settings."
                    .to_string()
            })?;
        let args = binary
            .and_then(|binary| binary.arguments)
            .unwrap_or_else(|| vec!["lsp".to_string()]);
        Ok(zed::Command { command, args, env: Vec::new() })
    }

    /// Forwards `lsp.asm-check.initialization_options`, e.g.
    /// `{"pedantic": true, "allow": ["unused-label"]}`.
    fn language_server_initialization_options(
        &mut self,
        id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        Ok(LspSettings::for_worktree(id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.initialization_options))
    }

    fn language_server_workspace_configuration(
        &mut self,
        id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        Ok(LspSettings::for_worktree(id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.settings))
    }
}

zed::register_extension!(AsmCheck);
