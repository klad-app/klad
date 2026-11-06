use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "shipped.json";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    bundle: String,
    about: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub name: String,
    pub about: String,
    pub path: PathBuf,
}

pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("KLAD_SAMPLES") {
        return Some(PathBuf::from(dir)).filter(|d| d.join(MANIFEST).is_file());
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [
        dir.join("samples"),
        dir.join("../Resources/samples"),
        dir.join("../share/klad/samples"),
    ]
    .into_iter()
    .chain(dir.ancestors().skip(1).take(5).map(|a| a.join("samples")))
    .find(|d| d.join(MANIFEST).is_file())
}

pub fn listed(dir: &Path) -> Result<Vec<Sample>> {
    let text =
        fs::read_to_string(dir.join(MANIFEST)).with_context(|| format!("reading {}", dir.join(MANIFEST).display()))?;
    let entries: Vec<Entry> = serde_json::from_str(&text).with_context(|| format!("{} does not parse", MANIFEST))?;
    Ok(entries
        .into_iter()
        .map(|e| Sample {
            name: e.name,
            about: e.about,
            path: dir.join(e.bundle),
        })
        .filter(|s| s.path.join("doc.json").is_file())
        .collect())
}

pub fn shipped() -> Vec<Sample> {
    dir().and_then(|d| listed(&d).ok()).unwrap_or_default()
}

pub fn free_path(folder: &Path, name: &str) -> PathBuf {
    (1..)
        .map(|i| match i {
            1 => folder.join(format!("{name}.klad")),
            n => folder.join(format!("{name} {n}.klad")),
        })
        .find(|p| !p.exists())
        .unwrap_or_else(|| folder.join(format!("{name}.klad")))
}

pub fn copy(sample: &Sample, folder: &Path) -> Result<PathBuf> {
    if !sample.path.join("doc.json").is_file() {
        bail!("the sample {} is not there: {}", sample.name, sample.path.display());
    }
    fs::create_dir_all(folder).with_context(|| format!("making {}", folder.display()))?;
    let to = free_path(folder, &sample.name);
    copy_dir(&sample.path, &to).with_context(|| format!("copying {} to {}", sample.name, to.display()))?;
    Ok(to)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    fs::read_dir(from)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() != ".cache")
        .try_for_each(|e| {
            let target = to.join(e.file_name());
            match e.file_type()?.is_dir() {
                true => copy_dir(&e.path(), &target),
                false => fs::copy(e.path(), target).map(|_| ()).map_err(Into::into),
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{Blank, Bundle};

    #[test]
    fn a_sample_is_copied_without_its_cache_under_a_free_name() {
        let dir = std::env::temp_dir().join(format!("klad-samples-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let shipped = dir.join("shipped");
        Bundle::blank(shipped.join("rock/rock.klad"), &Blank::default(), "cli").unwrap();
        fs::create_dir_all(shipped.join("rock/rock.klad/.cache")).unwrap();
        fs::write(shipped.join("rock/rock.klad/.cache/thumb.png"), b"x").unwrap();
        fs::write(
            shipped.join(MANIFEST),
            r#"[{"name":"rock","bundle":"rock/rock.klad","about":"a rock"},{"name":"gone","bundle":"gone/gone.klad","about":"not built"}]"#,
        )
        .unwrap();
        let found = listed(&shipped).unwrap();
        assert_eq!(found.len(), 1, "samples that are not built are left out");
        assert_eq!(found[0].about, "a rock");
        let mine = dir.join("mine");
        let first = copy(&found[0], &mine).unwrap();
        let second = copy(&found[0], &mine).unwrap();
        assert_eq!(first, mine.join("rock.klad"));
        assert_eq!(second, mine.join("rock 2.klad"));
        assert!(Bundle::open(&second).is_ok());
        assert!(!first.join(".cache/thumb.png").exists(), "the cache stays behind");
        fs::remove_dir_all(&dir).unwrap();
    }
}
