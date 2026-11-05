use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

fn on_path(dir: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir))
}

fn default_dir() -> Result<PathBuf> {
    let home = ["HOME", "USERPROFILE"]
        .iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .context("no home folder (HOME) to install into")?;
    Ok(match cfg!(target_os = "macos") {
        true => PathBuf::from("/usr/local/bin"),
        false => home.join(".local/bin"),
    })
}

#[cfg(unix)]
fn link(exe: &Path, dir: &Path) -> Result<PathBuf> {
    let to = dir.join("klad");
    let writable = std::fs::create_dir_all(dir).is_ok()
        && std::fs::symlink_metadata(&to).map_or(true, |_| std::fs::remove_file(&to).is_ok())
        && std::os::unix::fs::symlink(exe, &to).is_ok();
    if writable {
        return Ok(to);
    }
    if !cfg!(target_os = "macos") {
        bail!(
            "{} is not writable: give --dir with a folder of yours on the PATH",
            dir.display()
        );
    }
    let quoted = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "'\\\\''"));
    let script = format!(
        "do shell script \"mkdir -p {} && ln -sf {} {}\" with administrator privileges",
        quoted(dir),
        quoted(exe),
        quoted(&to)
    );
    let status = Command::new("osascript").args(["-e", &script]).status()?;
    if !status.success() {
        bail!(
            "the link in {} was not made (the password prompt was cancelled?)",
            dir.display()
        );
    }
    Ok(to)
}

#[cfg(windows)]
fn link(exe: &Path, _dir: &Path) -> Result<PathBuf> {
    let folder = exe.parent().context("klad.exe has no folder")?;
    if on_path(folder) {
        return Ok(exe.to_path_buf());
    }
    let script = format!(
        "$p=[Environment]::GetEnvironmentVariable('Path','User'); if (-not ($p -split ';' -contains '{0}')) {{ [Environment]::SetEnvironmentVariable('Path', (@($p,'{0}') | Where-Object {{ $_ }}) -join ';', 'User') }}",
        folder.display().to_string().replace('\'', "''")
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()?;
    if !status.success() {
        bail!("adding {} to your PATH failed", folder.display());
    }
    Ok(exe.to_path_buf())
}

pub fn install(dir: Option<&Path>) -> Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let dir = match dir {
        Some(d) => d.to_path_buf(),
        None => default_dir()?,
    };
    let made = link(&exe, &dir)?;
    match cfg!(windows) {
        true => println!("{} is on your PATH: open a new terminal and run klad", exe.display()),
        false => println!("{} -> {}", made.display(), exe.display()),
    }
    if !cfg!(windows) && !on_path(&dir) {
        println!(
            "{} is not on your PATH yet: add it in your shell's profile",
            dir.display()
        );
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn install_links_klad_into_a_folder_and_replaces_an_old_link() {
        let dir = std::env::temp_dir().join(format!("klad-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink("/nowhere", dir.join("klad")).unwrap();
        install(Some(&dir)).unwrap();
        let target = std::fs::read_link(dir.join("klad")).unwrap();
        assert_eq!(target, std::env::current_exe().unwrap().canonicalize().unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
