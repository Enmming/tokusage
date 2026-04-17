//! Inject/remove a `Stop` hook into `~/.claude/settings.json`.
//!
//! Each injected hook group carries a `_tokusage_managed: true` sentinel so we
//! can locate and remove exactly what we wrote. Other hook groups (user's
//! own or other tools') are preserved untouched.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const SENTINEL: &str = "_tokusage_managed";

pub fn settings_path() -> Result<PathBuf> {
    let dirs = directories::BaseDirs::new()
        .context("could not determine user home directory")?;
    Ok(dirs.home_dir().join(".claude/settings.json"))
}

pub fn inject(binary_path: &Path) -> Result<bool> {
    let path = settings_path()?;
    let mut settings: Value = if path.exists() {
        let text = fs::read_to_string(&path)?;
        serde_json::from_str(&text)
            .with_context(|| format!("parsing existing {}", path.display()))?
    } else {
        json!({})
    };

    if !settings.is_object() {
        anyhow::bail!("{} is not a JSON object", path.display());
    }

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        anyhow::bail!("hooks in {} is not an object", path.display());
    }

    let stop = hooks
        .as_object_mut()
        .unwrap()
        .entry("Stop")
        .or_insert_with(|| json!([]));
    if !stop.is_array() {
        anyhow::bail!("hooks.Stop in {} is not an array", path.display());
    }

    // Remove any pre-existing tokusage group to avoid duplicates when re-running init.
    let groups = stop.as_array_mut().unwrap();
    groups.retain(|g| !is_tokusage_group(g));

    let command = format!(
        "{} submit > /dev/null 2>&1 &",
        binary_path.display()
    );
    groups.push(json!({
        SENTINEL: true,
        "hooks": [
            {
                "type": "command",
                "command": command
            }
        ]
    }));

    fs::create_dir_all(path.parent().unwrap())?;
    let text = serde_json::to_string_pretty(&settings)?;
    fs::write(&path, text)?;
    Ok(true)
}

/// Remove any hook group bearing the `_tokusage_managed` sentinel. Returns
/// true if something was removed.
pub fn remove() -> Result<bool> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(false);
    }

    let text = fs::read_to_string(&path)?;
    let mut settings: Value = serde_json::from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))?;

    let Some(hooks) = settings
        .as_object_mut()
        .and_then(|obj| obj.get_mut("hooks"))
        .and_then(Value::as_object_mut)
    else {
        return Ok(false);
    };
    let Some(stop) = hooks.get_mut("Stop").and_then(Value::as_array_mut) else {
        return Ok(false);
    };

    let before = stop.len();
    stop.retain(|g| !is_tokusage_group(g));
    let removed = stop.len() < before;

    if stop.is_empty() {
        hooks.remove("Stop");
    }

    // Write back if anything changed.
    if removed {
        let text = serde_json::to_string_pretty(&settings)?;
        fs::write(&path, text)?;
    }
    Ok(removed)
}

fn is_tokusage_group(v: &Value) -> bool {
    v.as_object()
        .and_then(|o| o.get(SENTINEL))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_removes_then_reinjects_idempotently() {
        // This is a unit-ish test: we bypass settings_path() by temporarily
        // changing HOME so the real settings file isn't touched.
        let tmp = tempfile::TempDir::new().unwrap();
        let home = tmp.path();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::env::set_var("HOME", home);
        #[cfg(target_os = "macos")]
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));

        // Pre-existing hook belongs to the user; must survive.
        std::fs::write(
            home.join(".claude/settings.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo user-hook"}]}]}}"#,
        )
        .unwrap();

        let bin = home.join("bin/tokusage");
        inject(&bin).unwrap();

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(home.join(".claude/settings.json")).unwrap())
                .unwrap();
        let groups = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(groups.len(), 2, "user group + tokusage group");
        assert!(groups.iter().any(|g| g[SENTINEL].as_bool() == Some(true)));
        assert!(groups
            .iter()
            .any(|g| g["hooks"][0]["command"].as_str() == Some("echo user-hook")));

        // Re-inject should not duplicate.
        inject(&bin).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(home.join(".claude/settings.json")).unwrap())
                .unwrap();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);

        // Remove should only take tokusage group.
        let removed = remove().unwrap();
        assert!(removed);
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(home.join(".claude/settings.json")).unwrap())
                .unwrap();
        let groups = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0]["hooks"][0]["command"].as_str(),
            Some("echo user-hook")
        );
    }
}
