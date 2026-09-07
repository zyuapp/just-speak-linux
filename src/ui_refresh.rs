//! Refresh the live desktop after switching release files.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{fs, path::PathBuf, time::Duration};

pub fn refresh() -> Result<bool> {
    let configuration = crate::config::config_path()?;
    let root = configuration
        .parent()
        .and_then(|path| path.parent())
        .context("configuration directory is missing")?;
    let plugin = root.join("omarchy/plugins/local.just-speak");
    let prefix = std::env::var_os("JUST_SPEAK_PREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local")
        });
    let installed_here = fs::canonicalize(&plugin).ok().is_some_and(|path| {
        fs::canonicalize(prefix.join("share/just-speak/ui"))
            .ok()
            .as_ref()
            == Some(&path)
    });
    if !crate::executable_exists("omarchy") || !crate::executable_exists("omarchy-shell") {
        return Ok(false);
    }
    refresh_with(installed_here, |program, arguments| {
        crate::inputs::output(program, arguments, Duration::from_secs(45), 1024 * 1024)
    })
}

fn refresh_with(
    installed_here: bool,
    mut run: impl FnMut(&str, &[&str]) -> Result<Vec<u8>>,
) -> Result<bool> {
    if !installed_here {
        return Ok(false);
    }
    let bytes = run("omarchy-shell", &["shell", "listPlugins"])
        .context("read the running Omarchy plugin registry")?;
    let plugins: Value =
        serde_json::from_slice(&bytes).context("invalid Omarchy plugin registry")?;
    let plugins = plugins
        .as_array()
        .context("Omarchy plugin registry is not a list")?;
    if !plugins
        .iter()
        .any(|plugin| plugin["id"] == "local.just-speak" && plugin["enabled"] == true)
    {
        return Ok(false);
    }
    // Plugin rescans can preserve compiled components AND directory metadata.
    // The supported restart also refuses to disrupt a secure lock screen.
    run("omarchy", &["restart", "shell"]).context("restart Omarchy shell")?;
    let ready =
        run("omarchy-shell", &["shell", "ping"]).context("check restarted Omarchy shell")?;
    ensure!(
        ready.trim_ascii() == b"ok",
        "Omarchy shell did not report ready"
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;

    #[test]
    fn enabled_plugin_restarts_engine_and_checks_readiness() {
        let mut calls = Vec::new();
        assert!(
            refresh_with(true, |program, args| {
                calls.push((
                    program.to_owned(),
                    args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                ));
                match args {
                    ["shell", "listPlugins"] => Ok(
                        br#"[{"id":"local.just-speak","enabled":true,"active":false}]"#.to_vec(),
                    ),
                    ["restart", "shell"] => Ok(Vec::new()),
                    ["shell", "ping"] => Ok(b"ok\n".to_vec()),
                    _ => bail!("unexpected command"),
                }
            })
            .unwrap()
        );
        assert_eq!(
            calls
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["omarchy-shell", "omarchy", "omarchy-shell"]
        );
    }

    #[test]
    fn absent_or_disabled_integration_does_not_restart_shell() {
        assert!(
            !refresh_with(false, |_, _| panic!(
                "unrelated desktop must not be contacted"
            ))
            .unwrap()
        );
        assert!(
            !refresh_with(true, |_, args| {
                assert_eq!(args, ["shell", "listPlugins"]);
                Ok(br#"[{"id":"local.just-speak","enabled":false}]"#.to_vec())
            })
            .unwrap()
        );
    }

    #[test]
    fn failed_restart_and_bad_readiness_are_reported() {
        for fail_restart in [true, false] {
            assert!(
                refresh_with(true, |_, args| match args {
                    ["shell", "listPlugins"] =>
                        Ok(br#"[{"id":"local.just-speak","enabled":true}]"#.to_vec()),
                    ["restart", "shell"] if fail_restart => bail!("session is locked"),
                    ["restart", "shell"] => Ok(Vec::new()),
                    ["shell", "ping"] => Ok(b"not ready".to_vec()),
                    _ => bail!("unexpected command"),
                })
                .is_err()
            );
        }
        assert!(refresh_with(true, |_, _| Ok(b"invalid registry".to_vec())).is_err());
    }
}
