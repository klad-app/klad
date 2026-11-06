use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FEED: &str = "https://api.github.com/repos/klad-app/klad/releases/latest";
const DAY: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub notes: String,
}

#[derive(Deserialize)]
struct Feed {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
}

pub fn parse(json: &str) -> Option<Release> {
    let feed: Feed = serde_json::from_str(json).ok()?;
    Some(Release {
        version: feed.tag_name.trim_start_matches('v').to_string(),
        url: feed.html_url,
        notes: feed.body.unwrap_or_default(),
    })
}

fn parts(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(['.', '-', '+', ' '])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

pub fn newer(current: &str, latest: &str) -> bool {
    parts(latest) > parts(current)
}

pub fn due(enabled: bool, last: u64, now: u64) -> bool {
    enabled && now.saturating_sub(last) >= DAY
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub type Checked = Result<Option<Release>, String>;

fn answer(code: &str, body: &str) -> Result<String, String> {
    match code.trim() {
        "200" => Ok(body.to_string()),
        "404" => Err("no release of klad is published yet".to_string()),
        other => Err(format!("the releases feed answered {other}")),
    }
}

fn fetch(feed: &str) -> Result<String, String> {
    if !feed.starts_with("http") {
        return std::fs::read_to_string(feed).map_err(|e| format!("reading {feed}: {e}"));
    }
    let out = std::process::Command::new("curl")
        .args([
            "-sSL",
            "--max-time",
            "10",
            "-H",
            "Accept: application/vnd.github+json",
            "-w",
            "\n%{http_code}",
            feed,
        ])
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !out.status.success() {
        return Err("could not reach the releases feed (no network?)".to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    answer(code, body)
}

pub fn check(feed: &str, current: &str) -> Checked {
    let release = parse(&fetch(feed)?).ok_or_else(|| "the releases feed did not make sense".to_string())?;
    Ok(Some(release).filter(|r| newer(current, &r.version)))
}

pub fn summary(release: &Release) -> String {
    let first = release
        .notes
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '#']).trim())
        .find(|l| !l.is_empty());
    match first {
        Some(line) => format!("klad {} is out: {line}", release.version),
        None => format!("klad {} is out", release.version),
    }
}

pub fn outcome(checked: &Checked) -> String {
    match checked {
        Ok(Some(release)) => summary(release),
        Ok(None) => format!("klad {} is the newest version", env!("CARGO_PKG_VERSION")),
        Err(e) => format!("could not check: {e}"),
    }
}

pub fn spawn(feed: String) -> std::sync::mpsc::Receiver<Checked> {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("klad update check".into())
        .spawn(move || {
            let _ = send.send(check(&feed, env!("CARGO_PKG_VERSION")));
        })
        .ok();
    receive
}

pub fn feed() -> String {
    std::env::var("KLAD_UPDATE_FEED").unwrap_or_else(|_| FEED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_release_in_the_feed_is_found_once_a_day_and_never_when_off() {
        let dir = std::env::temp_dir().join(format!("klad-updates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let feed = dir.join("latest.json");
        let write = |tag: &str| {
            std::fs::write(
                &feed,
                format!(
                    r#"{{"tag_name":"{tag}","html_url":"https://github.com/klad-app/klad/releases/tag/{tag}","body":"\n- Paint over: brushwork from any picture\n- more"}}"#
                ),
            )
            .unwrap()
        };
        write("v0.2.0");
        let found = check(feed.to_str().unwrap(), "0.1.0").unwrap().unwrap();
        assert_eq!(found.version, "0.2.0");
        assert_eq!(
            summary(&found),
            "klad 0.2.0 is out: Paint over: brushwork from any picture"
        );
        write("v0.1.0");
        assert_eq!(check(feed.to_str().unwrap(), "0.1.0"), Ok(None));
        assert_eq!(
            outcome(&Ok(None)),
            format!("klad {} is the newest version", env!("CARGO_PKG_VERSION"))
        );
        let missing = check(dir.join("missing.json").to_str().unwrap(), "0.1.0");
        assert!(
            outcome(&missing).starts_with("could not check: reading "),
            "{missing:?}"
        );
        std::fs::write(&feed, "<html>").unwrap();
        assert_eq!(
            check(feed.to_str().unwrap(), "0.1.0"),
            Err("the releases feed did not make sense".to_string())
        );
        assert_eq!(
            answer("404", ""),
            Err("no release of klad is published yet".to_string())
        );
        assert_eq!(answer("503", ""), Err("the releases feed answered 503".to_string()));
        assert_eq!(answer("200", "{}"), Ok("{}".to_string()));
        assert!(newer("0.9.3", "0.10.0") && !newer("1.2.0", "1.1.9") && newer("0.1.0", "v0.1.1"));
        assert!(due(true, 0, DAY) && !due(true, DAY, DAY + 60) && !due(false, 0, 10 * DAY));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_check_runs_beside_the_ui() {
        let dir = std::env::temp_dir().join(format!("klad-updates-bg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let feed = dir.join("latest.json");
        std::fs::write(&feed, r#"{"tag_name":"v99.0.0","html_url":"https://example.org"}"#).unwrap();
        let checking = spawn(feed.display().to_string());
        let found = checking.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(found.unwrap().map(|r| r.version), Some("99.0.0".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
