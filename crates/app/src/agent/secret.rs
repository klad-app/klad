use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const SERVICE: &str = "klad: Claude API key";
const ACCOUNT: &str = "klad";

fn file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("KLAD_AGENT_KEY_FILE") {
        return Some(PathBuf::from(path));
    }
    crate::places::config().map(|c| c.join("klad").join("agent-key"))
}

fn keychain() -> bool {
    cfg!(target_os = "macos") && std::env::var_os("KLAD_AGENT_KEY_FILE").is_none()
}

pub fn store(key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() || key.contains(['\n', '"', ' ']) {
        bail!("that does not look like an API key");
    }
    if keychain() {
        let mut child = Command::new("security")
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("starting security for the Keychain")?;
        writeln!(
            child.stdin.take().context("security's input")?,
            "add-generic-password -U -a \"{ACCOUNT}\" -s \"{SERVICE}\" -w \"{key}\""
        )?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!(
                "the Keychain refused the key: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        return Ok(());
    }
    let path = file().context("no settings folder for the key")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn load() -> Option<String> {
    if keychain() {
        let out = Command::new("security")
            .args(["find-generic-password", "-a", ACCOUNT, "-s", SERVICE, "-w"])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let key = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return (out.status.success() && !key.is_empty()).then_some(key);
    }
    std::fs::read_to_string(file()?)
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

pub fn forget() -> Result<()> {
    if keychain() {
        let _ = Command::new("security")
            .args(["delete-generic-password", "-a", ACCOUNT, "-s", SERVICE])
            .output();
        return Ok(());
    }
    if let Some(path) = file().filter(|p| p.exists()) {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn a_key_is_kept_out_of_settings_in_a_file_only_its_owner_reads() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("klad-key-{}", std::process::id()));
        unsafe { std::env::set_var("KLAD_AGENT_KEY_FILE", &path) };
        store("sk-ant-test-123").unwrap();
        assert_eq!(load().as_deref(), Some("sk-ant-test-123"));
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(store("not a key").is_err());
        forget().unwrap();
        assert!(load().is_none());
        unsafe { std::env::remove_var("KLAD_AGENT_KEY_FILE") };
    }
}
