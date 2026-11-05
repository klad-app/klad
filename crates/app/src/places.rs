use std::path::PathBuf;

pub fn home() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .iter()
        .find_map(std::env::var_os)
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

pub fn config() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join("AppData/Roaming")));
    }
    let home = home()?;
    Some(match cfg!(target_os = "macos") {
        true => home.join("Library/Application Support"),
        false => std::env::var_os("XDG_CONFIG_HOME").map_or(home.join(".config"), PathBuf::from),
    })
}

pub fn drawings() -> PathBuf {
    home().map_or_else(|| PathBuf::from("Drawings"), |h| h.join("Drawings"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_follow_the_platform() {
        let config = config().unwrap();
        let home = home().unwrap();
        match cfg!(target_os = "macos") {
            true => assert_eq!(config, home.join("Library/Application Support")),
            false if cfg!(target_os = "windows") => {
                assert!(config.ends_with("Roaming") || std::env::var_os("APPDATA").is_some())
            }
            false => assert!(config.ends_with(".config") || std::env::var_os("XDG_CONFIG_HOME").is_some()),
        }
        assert_eq!(drawings(), home.join("Drawings"));
    }
}
