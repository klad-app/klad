use anyhow::{Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const ISSUES: &str = "https://github.com/klad-app/klad/issues/new";
const LOG_LIMIT: u64 = 512 * 1024;
pub const LOG_LINES: usize = 40;

#[derive(Debug)]
pub struct Bug(pub String);

impl std::fmt::Display for Bug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Bug {}

pub fn io_words(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "it is not there (moved or deleted?)".to_string(),
        std::io::ErrorKind::PermissionDenied => "klad may not read it: check the file's permissions".to_string(),
        _ => e.to_string(),
    }
}

pub fn job_failed(name: &str, why: &str) -> String {
    let why = why.trim().trim_start_matches("error: ");
    match why.split_once("klad hit a bug: ") {
        Some((_, rest)) => {
            let what = rest.split(". KLAD_BACKTRACE").next().unwrap_or(rest);
            format!(
                "something went wrong inside klad ({name}: {what}). Everything up to your last edit is saved; Help > Report a problem sends it to {} or to klad's issues",
                klad_core::contact::SUPPORT
            )
        }
        None => format!("{name} did not finish: {why}"),
    }
}

pub fn bug(what: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Bug(what.into()))
}

pub fn is_bug(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<Bug>().is_some())
}

pub fn bug_message(e: &anyhow::Error) -> String {
    format!(
        "something went wrong inside klad ({e:#}). Everything up to your last edit is saved; Help > Report a problem sends it to {} or to klad's issues",
        klad_core::contact::SUPPORT
    )
}

#[cfg(test)]
thread_local! {
    static TEST_LOG: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn log_to(path: Option<PathBuf>) {
    TEST_LOG.with(|t| *t.borrow_mut() = path);
}

pub fn log_path() -> Option<PathBuf> {
    #[cfg(test)]
    let chosen = TEST_LOG.with(|t| t.borrow().clone());
    #[cfg(not(test))]
    let chosen = std::env::var_os("KLAD_LOG")
        .map(PathBuf::from)
        .or_else(|| crate::places::config().map(|c| c.join("klad").join("klad.log")));
    chosen
}

pub fn log(line: &str) {
    let Some(path) = log_path() else {
        return;
    };
    if fs::metadata(&path).is_ok_and(|m| m.len() > LOG_LIMIT) {
        let kept = fs::read_to_string(&path).unwrap_or_default();
        let half = kept.len() / 2;
        let from = kept[half..].find('\n').map_or(kept.len(), |i| half + i + 1);
        let _ = fs::write(&path, &kept[from..]);
    }
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {}", stamp(crate::updates::now()), line.replace('\n', " "));
    }
}

pub fn last_lines(n: usize) -> Vec<String> {
    let text = log_path().and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

pub fn stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

pub fn system() -> String {
    let name = match std::env::consts::OS {
        "macos" => output("sw_vers", &["-productVersion"]).map(|v| format!("macOS {v}")),
        "linux" => fs::read_to_string("/etc/os-release").ok().and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_string())
        }),
        "windows" => output("cmd", &["/c", "ver"]),
        _ => None,
    }
    .unwrap_or_else(|| std::env::consts::OS.to_string());
    format!("{name} on {}", std::env::consts::ARCH)
}

pub fn private(text: &str, folder: Option<&Path>) -> String {
    let hidden = folder
        .map(|f| (f.display().to_string(), "<the drawing's folder>"))
        .into_iter()
        .chain(crate::places::home().map(|h| (h.display().to_string(), "~")))
        .filter(|(from, _)| from.len() > 1);
    hidden.fold(text.to_string(), |t, (from, to)| t.replace(&from, to))
}

fn count(n: usize, one: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {one}s"),
    }
}

pub fn summary(doc: &klad_core::doc::Document) -> String {
    format!(
        "{} by {} px, {}, {}",
        doc.canvas.width,
        doc.canvas.height,
        count(doc.all_layers().len(), "layer"),
        count(doc.timeline.frames as usize, "frame")
    )
}

pub fn panic_text(info: &std::panic::PanicHookInfo) -> String {
    let what = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_default();
    let place = info
        .location()
        .map(|l| format!(" at {}:{}", l.file(), l.line()))
        .unwrap_or_default();
    format!("{}{place}", what.replace('\n', " "))
}

pub struct Report {
    pub from: &'static str,
    pub folder: Option<PathBuf>,
    pub drawing: Option<String>,
    pub attached: Option<String>,
    pub messages: Vec<String>,
    pub log: Vec<String>,
}

impl Report {
    pub fn text(&self) -> String {
        let mut out = vec![
            format!("klad {}, {}", crate::VERSION, self.from),
            system(),
            String::new(),
            "What I did, what I expected and what happened instead:".to_string(),
            String::new(),
            String::new(),
        ];
        if let Some(drawing) = &self.drawing {
            out.push(match &self.attached {
                Some(file) => format!("The drawing ({drawing}) is attached as {file}."),
                None => format!("The drawing ({drawing}) is not attached."),
            });
        }
        [
            ("Last messages:", &self.messages),
            ("Last lines of klad's log:", &self.log),
        ]
        .into_iter()
        .filter(|(_, lines)| !lines.is_empty())
        .for_each(|(title, lines)| {
            out.push(String::new());
            out.push(title.to_string());
            out.extend(lines.iter().map(|l| private(l, self.folder.as_deref())));
        });
        out.join("\n") + "\n"
    }
}

fn files(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    entries
        .into_iter()
        .filter(|e| e.file_name() != ".cache")
        .try_for_each(|e| {
            let name = format!("{prefix}/{}", e.file_name().to_string_lossy());
            match e.file_type()?.is_dir() {
                true => files(&e.path(), &name, out),
                false => {
                    out.push((name, e.path()));
                    Ok(())
                }
            }
        })
}

pub fn zip_drawing(bundle: &Path, to: &Path) -> Result<()> {
    let root = bundle
        .file_name()
        .map_or_else(|| "drawing.klad".to_string(), |n| n.to_string_lossy().to_string());
    let mut listed = Vec::new();
    files(bundle, &root, &mut listed)?;
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let put16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let put32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    listed.iter().try_for_each(|(name, path)| -> Result<()> {
        let data = fs::read(path)?;
        let (Ok(size), Ok(offset)) = (u32::try_from(data.len()), u32::try_from(out.len())) else {
            bail!("the drawing is too big to zip for a report; attach its folder another way");
        };
        let crc = crc32fast::hash(&data);
        let header = |v: &mut Vec<u8>| {
            put16(v, 20);
            put16(v, 0x0800);
            put16(v, 0);
            put16(v, 0);
            put16(v, 0x21);
            put32(v, crc);
            put32(v, size);
            put32(v, size);
            put16(v, name.len() as u16);
            put16(v, 0);
        };
        put32(&mut out, 0x0403_4b50);
        header(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data);
        put32(&mut central, 0x0201_4b50);
        put16(&mut central, 20);
        header(&mut central);
        [0u16, 0, 0].iter().for_each(|x| put16(&mut central, *x));
        put32(&mut central, 0);
        put32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
        Ok(())
    })?;
    let (Ok(start), Ok(length), Ok(count)) = (
        u32::try_from(out.len()),
        u32::try_from(central.len()),
        u16::try_from(listed.len()),
    ) else {
        bail!("the drawing is too big to zip for a report; attach its folder another way");
    };
    out.extend_from_slice(&central);
    put32(&mut out, 0x0605_4b50);
    [0u16, 0, count, count].iter().for_each(|x| put16(&mut out, *x));
    put32(&mut out, length);
    put32(&mut out, start);
    put16(&mut out, 0);
    fs::write(to, out)?;
    Ok(())
}

pub fn zip_beside(bundle: &Path) -> Result<PathBuf> {
    let stem = bundle
        .file_stem()
        .map_or_else(|| "drawing".to_string(), |s| s.to_string_lossy().to_string());
    let to = bundle.with_file_name(format!("{stem} for a report.zip"));
    zip_drawing(bundle, &to)?;
    Ok(to)
}

#[cfg(test)]
pub fn unzip_names(bytes: &[u8]) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    let mut at = 0;
    while bytes.len() >= at + 30 && bytes[at..at + 4] == 0x0403_4b50u32.to_le_bytes() {
        let read32 = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let read16 = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
        let (crc, size, name_len) = (read32(at + 14), read32(at + 18) as usize, read16(at + 26));
        let name = String::from_utf8_lossy(&bytes[at + 30..at + 30 + name_len]).to_string();
        let data = &bytes[at + 30 + name_len..at + 30 + name_len + size];
        found.push((name, crc32fast::hash(data) == crc));
        at += 30 + name_len + size;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_job_says_why_and_a_crash_points_at_the_report() {
        assert_eq!(
            job_failed("export to Godot", "error: no clip run in art.klad"),
            "export to Godot did not finish: no clip run in art.klad"
        );
        let crashed = job_failed(
            "pose at frame 4",
            "error: klad hit a bug: index out of bounds at crates/core/src/rig.rs:10. KLAD_BACKTRACE=1 shows the details; please report it to hello@klad.studio or at https://github.com/klad-app/klad/issues",
        );
        assert!(crashed.starts_with(
            "something went wrong inside klad (pose at frame 4: index out of bounds at crates/core/src/rig.rs:10)"
        ));
        assert!(!crashed.contains("KLAD_BACKTRACE") && crashed.contains("Help > Report a problem"));
    }

    #[test]
    fn dates_read_as_dates() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00");
        assert_eq!(stamp(1_793_986_200), "2026-11-06 17:30:00");
    }

    #[test]
    fn the_log_keeps_its_last_lines_and_stays_small() {
        let dir = std::env::temp_dir().join(format!("klad-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        log_to(Some(dir.join("klad.log")));
        (0..20).for_each(|i| log(&format!("line {i}\nwrapped")));
        let last = last_lines(3);
        assert_eq!(last.len(), 3);
        assert!(last[2].ends_with("line 19 wrapped"), "{last:?}");
        fs::write(dir.join("klad.log"), "x\n".repeat(400_000)).unwrap();
        log("after");
        assert!(fs::metadata(dir.join("klad.log")).unwrap().len() < LOG_LIMIT);
        assert!(last_lines(1)[0].ends_with("after"));
        log_to(None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_drawing_zips_without_its_cache_and_every_file_checks_out() {
        let dir = std::env::temp_dir().join(format!("klad-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let bundle = dir.join("rock.klad");
        klad_core::bundle::Bundle::blank(&bundle, &klad_core::bundle::Blank::default(), "cli").unwrap();
        fs::create_dir_all(bundle.join(".cache")).unwrap();
        fs::write(bundle.join(".cache/thumb.png"), b"x").unwrap();
        let zip = zip_beside(&bundle).unwrap();
        assert_eq!(zip, dir.join("rock for a report.zip"));
        let bytes = fs::read(&zip).unwrap();
        let names = unzip_names(&bytes);
        assert!(
            names.iter().any(|(n, ok)| n == "rock.klad/doc.json" && *ok),
            "{names:?}"
        );
        assert!(names.iter().all(|(n, ok)| *ok && !n.contains(".cache")));
        assert_eq!(
            &bytes[bytes.len() - 22..bytes.len() - 18],
            &0x0605_4b50u32.to_le_bytes()
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
