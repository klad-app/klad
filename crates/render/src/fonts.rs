use crate::assets;
use crate::text::weight_axis;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

pub const MANIFEST: &str = "fonts.json";
pub const CATALOGUE: &str = "catalogue.json";
pub const FAMILY: &str = "family.json";
pub const GROUPS: [&str; 6] = ["sans", "serif", "mono", "display", "handwriting", "pixel"];
const RAW: &str = "https://raw.githubusercontent.com/google/fonts";
pub const WEIGHTS: [(f32, &str); 10] = [
    (100.0, "Thin"),
    (200.0, "ExtraLight"),
    (300.0, "Light"),
    (400.0, "Regular"),
    (500.0, "Medium"),
    (600.0, "SemiBold"),
    (700.0, "Bold"),
    (800.0, "ExtraBold"),
    (900.0, "Black"),
    (1000.0, "ExtraBlack"),
];

static INSTALLED: Mutex<Option<Vec<Family>>> = Mutex::new(None);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Face {
    pub file: String,
    pub style: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Family {
    pub family: String,
    pub group: String,
    pub licence: String,
    pub folder: String,
    pub licence_file: String,
    #[serde(default)]
    pub copyright: String,
    pub files: Vec<Face>,
    #[serde(skip)]
    pub dir: PathBuf,
    #[serde(skip)]
    pub shipped: bool,
}

impl Family {
    pub fn regular(&self) -> &Face {
        self.files
            .iter()
            .find(|f| f.style == "Regular")
            .or(self.files.first())
            .expect("a family has at least one file")
    }

    pub fn path(&self, face: &Face) -> PathBuf {
        self.dir.join(&face.file)
    }
}

#[derive(Deserialize)]
struct Manifest {
    families: Vec<Family>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Listed {
    pub family: String,
    pub group: String,
    pub path: String,
    #[serde(default)]
    pub rank: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Catalogue {
    pub commit: String,
    pub families: Vec<Listed>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub family: Family,
    pub face: Face,
    pub weight: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chosen {
    pub file: String,
    pub weight: Option<f32>,
    pub fetched: Option<String>,
}

pub fn key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn is_font(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".ttf", ".otf", ".ttc"].iter().any(|e| lower.ends_with(e))
}

pub fn shipped_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("KLAD_FONTS") {
        return Some(PathBuf::from(dir)).filter(|d| d.join(MANIFEST).is_file());
    }
    found_from(&std::env::current_exe().ok()?)
}

fn found_from(exe: &Path) -> Option<PathBuf> {
    let real = exe.canonicalize().ok();
    std::iter::once(exe.to_path_buf())
        .chain(real)
        .filter_map(|e| e.parent().map(Path::to_path_buf))
        .flat_map(|dir| {
            let near = [
                dir.join("fonts"),
                dir.join("../Resources/fonts"),
                dir.join("../share/klad/fonts"),
            ];
            let above: Vec<PathBuf> = dir.ancestors().skip(1).take(5).map(|a| a.join("fonts")).collect();
            near.into_iter().chain(above)
        })
        .find(|d| d.join(MANIFEST).is_file())
}

pub fn user_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("KLAD_USER_FONTS") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("klad").join("fonts"));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let data = match cfg!(target_os = "macos") {
        true => home.join("Library/Application Support"),
        false => std::env::var_os("XDG_DATA_HOME").map_or(home.join(".local/share"), PathBuf::from),
    };
    Some(data.join("klad").join("fonts"))
}

fn shipped() -> Vec<Family> {
    let Some(dir) = shipped_dir() else {
        return Vec::new();
    };
    std::fs::read(dir.join(MANIFEST))
        .ok()
        .and_then(|b| serde_json::from_slice::<Manifest>(&b).ok())
        .map(|m| {
            m.families
                .into_iter()
                .map(|f| Family {
                    dir: dir.join(&f.folder),
                    shipped: true,
                    ..f
                })
                .collect()
        })
        .unwrap_or_default()
}

fn downloaded(taken: &[Family]) -> Vec<Family> {
    let Some(dir) = user_dir() else {
        return Vec::new();
    };
    let mut found: Vec<Family> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let bytes = std::fs::read(e.path().join(FAMILY)).ok()?;
                    let family: Family = serde_json::from_slice(&bytes).ok()?;
                    Some(Family {
                        dir: e.path(),
                        shipped: false,
                        ..family
                    })
                })
                .filter(|f| !taken.iter().any(|t| key(&t.family) == key(&f.family)))
                .collect()
        })
        .unwrap_or_default();
    found.sort_by_key(|f| f.family.to_lowercase());
    found
}

pub fn installed() -> Vec<Family> {
    let mut held = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
    held.get_or_insert_with(|| {
        let shipped = shipped();
        let more = downloaded(&shipped);
        shipped.into_iter().chain(more).collect()
    })
    .clone()
}

pub fn forget() {
    *INSTALLED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn find(name: &str) -> Option<Pick> {
    let all = installed();
    let wanted = key(name);
    if let Some(pick) = all.iter().find_map(|f| {
        f.files
            .iter()
            .find(|x| x.file.eq_ignore_ascii_case(name.trim()))
            .map(|face| Pick {
                family: f.clone(),
                face: face.clone(),
                weight: None,
            })
    }) {
        return Some(pick);
    }
    let family = all
        .iter()
        .filter(|f| wanted.starts_with(&key(&f.family)))
        .max_by_key(|f| key(&f.family).len())?;
    let rest = &wanted[key(&family.family).len()..];
    let pick = |face: &Face, weight| Pick {
        family: family.clone(),
        face: face.clone(),
        weight,
    };
    if rest.is_empty() || rest == "regular" {
        return Some(pick(family.regular(), None));
    }
    if let Some(face) = family.files.iter().find(|x| key(&x.style) == rest) {
        return Some(pick(face, None));
    }
    let weight = WEIGHTS.iter().find(|(_, n)| key(n) == rest)?.0;
    let face = family.regular();
    let bytes = std::fs::read(family.path(face)).ok()?;
    weight_axis(&bytes).map(|_| pick(face, Some(weight)))
}

pub fn catalogue() -> Result<Catalogue> {
    let dir =
        shipped_dir().ok_or_else(|| anyhow!("klad's fonts folder is missing, so the open fonts cannot be listed"))?;
    let bytes =
        std::fs::read(dir.join(CATALOGUE)).with_context(|| format!("reading {}", dir.join(CATALOGUE).display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn search(words: &str) -> Result<Vec<Listed>> {
    let words: Vec<String> = words.split_whitespace().map(key).filter(|w| !w.is_empty()).collect();
    let mut found: Vec<Listed> = catalogue()?
        .families
        .into_iter()
        .filter(|f| {
            let hay = format!("{} {}", key(&f.family), f.group);
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect();
    let whole = words.concat();
    found.sort_by_key(|f| (key(&f.family) != whole, f.rank));
    Ok(found)
}

pub fn listed(name: &str) -> Option<Listed> {
    let wanted = key(name);
    catalogue()
        .ok()?
        .families
        .into_iter()
        .filter(|f| wanted.starts_with(&key(&f.family)))
        .max_by_key(|f| key(&f.family).len())
}

fn fetch(url: &str, out: Option<&Path>) -> Result<Vec<u8>> {
    let mut command = Command::new("curl");
    command.args(["-fsSL", "-g", "--retry", "2", "--max-time", "120"]);
    if let Some(out) = out {
        command.arg("-o").arg(out);
    }
    let done = command
        .arg(url)
        .output()
        .map_err(|e| anyhow!("downloading fonts needs curl, which did not start: {e}"))?;
    if !done.status.success() {
        bail!(
            "could not download {url}: {}",
            String::from_utf8_lossy(&done.stderr).trim()
        );
    }
    Ok(done.stdout)
}

struct Entry {
    filename: String,
    style: String,
    copyright: String,
}

fn entries(metadata: &str) -> Vec<Entry> {
    let field = |block: &str, name: &str| {
        block
            .lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix(&format!("{name}: ")))
            .map(|v| {
                v.strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .unwrap_or(v)
                    .replace("\\\"", "\"")
            })
            .unwrap_or_default()
    };
    metadata
        .split("\nfonts {")
        .skip(1)
        .filter_map(|rest| rest.split("\n}").next())
        .map(|block| {
            let weight: f32 = field(block, "weight").parse().unwrap_or(400.0);
            let name = WEIGHTS
                .iter()
                .find(|(w, _)| *w == weight)
                .map_or(format!("{weight}"), |(_, n)| n.to_string());
            let style = match (field(block, "style") == "italic", name.as_str()) {
                (true, "Regular") => "Italic".to_string(),
                (true, _) => format!("{name} Italic"),
                (false, _) => name,
            };
            Entry {
                filename: field(block, "filename"),
                style,
                copyright: field(block, "copyright"),
            }
        })
        .filter(|e| !e.filename.is_empty())
        .collect()
}

fn clean(filename: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    filename.chars().for_each(|c| match c {
        '[' => inside = true,
        ']' => inside = false,
        c if !inside => out.push(c),
        _ => {}
    });
    out
}

pub fn download(name: &str) -> Result<Family> {
    let catalogue = catalogue()?;
    let entry = catalogue
        .families
        .iter()
        .find(|f| key(&f.family) == key(name))
        .ok_or_else(|| anyhow!("no open font family called {name}; klad fonts --search finds one"))?;
    let (licence, licence_file) = match entry.path.split('/').next() {
        Some("ofl") => ("OFL-1.1", "OFL.txt"),
        Some("apache") => ("Apache-2.0", "LICENSE.txt"),
        _ => bail!("{} is not under the Open Font License or Apache 2.0", entry.family),
    };
    let folder = entry.path.rsplit('/').next().unwrap_or(&entry.path).to_string();
    let base = format!("{RAW}/{}/{}", catalogue.commit, entry.path);
    let metadata = String::from_utf8_lossy(&fetch(&format!("{base}/METADATA.pb"), None)?).to_string();
    let found = entries(&metadata);
    if found.is_empty() {
        bail!("{} lists no font files", entry.family);
    }
    let user = user_dir().ok_or_else(|| anyhow!("no folder for downloaded fonts (HOME is not set)"))?;
    let part = user.join(format!("{folder}.part"));
    let _ = std::fs::remove_dir_all(&part);
    std::fs::create_dir_all(&part).with_context(|| format!("making {}", part.display()))?;
    fetch(&format!("{base}/{licence_file}"), Some(&part.join(licence_file)))?;
    let files = found
        .iter()
        .map(|e| {
            let file = clean(&e.filename);
            let path = part.join(&file);
            fetch(&format!("{base}/{}", e.filename), Some(&path))?;
            crate::text::parse(&std::fs::read(&path)?)
                .map_err(|err| anyhow!("{file} is not a font this can read: {err}"))?;
            Ok(Face {
                file,
                style: e.style.clone(),
            })
        })
        .collect::<Result<Vec<Face>>>()?;
    let family = Family {
        family: entry.family.clone(),
        group: entry.group.clone(),
        licence: licence.to_string(),
        folder: folder.clone(),
        licence_file: licence_file.to_string(),
        copyright: found[0].copyright.clone(),
        files,
        dir: user.join(&folder),
        shipped: false,
    };
    std::fs::write(part.join(FAMILY), serde_json::to_string_pretty(&family)?)?;
    let _ = std::fs::remove_dir_all(&family.dir);
    std::fs::rename(&part, &family.dir)?;
    forget();
    Ok(family)
}

pub fn bring(root: &Path, name: &str) -> Result<Chosen> {
    if is_font(name) && root.join(assets::FONTS).join(name).is_file() {
        return Ok(Chosen {
            file: name.to_string(),
            weight: None,
            fetched: None,
        });
    }
    let (pick, fetched) = match find(name) {
        Some(pick) => (pick, None),
        None => {
            let listed = listed(name).ok_or_else(|| {
                anyhow!("no font called {name}: klad fonts lists the ones klad has, klad fonts --search {name} the open fonts it can download")
            })?;
            download(&listed.family)?;
            let pick = find(name).ok_or_else(|| anyhow!("{} was downloaded but has no style {name}", listed.family))?;
            (pick, Some(listed.family))
        }
    };
    let file = assets::import_font(root, &pick.family.path(&pick.face))?;
    let stem = file.rsplit_once('.').map_or(file.as_str(), |(s, _)| s);
    let licence_path = root.join(assets::FONTS).join(format!("{stem}.licence.txt"));
    if !licence_path.exists() {
        let text = std::fs::read_to_string(pick.family.dir.join(&pick.family.licence_file)).unwrap_or_default();
        let head = format!(
            "{} ({}), {}.\n{}\n\n",
            pick.family.family, pick.face.style, pick.family.licence, pick.family.copyright
        );
        klad_core::bundle::write_atomic(&licence_path, format!("{head}{text}").as_bytes())?;
    }
    Ok(Chosen {
        file,
        weight: pick.weight,
        fetched,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_gives_every_file_with_its_style() {
        let text = "name: \"Lobster\"\nfonts {\n  name: \"Lobster\"\n  style: \"normal\"\n  weight: 400\n  filename: \"Lobster-Regular.ttf\"\n  copyright: \"Copyright 2010 \\\"Impallari\\\"\"\n}\nfonts {\n  style: \"italic\"\n  weight: 700\n  filename: \"Inter-Italic[opsz,wght].ttf\"\n}\nsubsets: \"latin\"\n";
        let found = entries(text);
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].filename.as_str(), found[0].style.as_str()),
            ("Lobster-Regular.ttf", "Regular")
        );
        assert_eq!(found[0].copyright, "Copyright 2010 \"Impallari\"");
        assert_eq!(found[1].style, "Bold Italic");
        assert_eq!(clean(&found[1].filename), "Inter-Italic.ttf");
    }

    #[test]
    fn the_shipped_fonts_are_found_by_family_style_and_weight() {
        let all = installed();
        assert!(all.len() >= 50, "{} families", all.len());
        assert!(
            all.iter()
                .all(|f| f.shipped && f.files.iter().all(|x| f.path(x).is_file()))
        );
        assert!(
            all.iter().all(|f| GROUPS.contains(&f.group.as_str())),
            "every family has a known group"
        );
        let inter = find("Inter").unwrap();
        assert_eq!((inter.face.file.as_str(), inter.weight), ("Inter.ttf", None));
        assert_eq!(find("inter bold").unwrap().weight, Some(700.0));
        let barlow = find("Barlow Bold").unwrap();
        assert_eq!((barlow.face.file.as_str(), barlow.weight), ("Barlow-Bold.ttf", None));
        assert_eq!(find("Press Start 2P").unwrap().face.file, "PressStart2P-Regular.ttf");
        assert_eq!(
            find("PressStart2P-Regular.ttf").unwrap().family.family,
            "Press Start 2P"
        );
        assert!(find("Barlow Wobbly").is_none());
        assert!(find("Nope Sans").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn fonts_are_found_in_the_app_when_klad_runs_through_a_link() {
        let root = std::env::temp_dir().join(format!("klad-fonts-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (bin, fonts, path) = (
            root.join("klad.app/Contents/MacOS"),
            root.join("klad.app/Contents/Resources/fonts"),
            root.join("bin"),
        );
        [&bin, &fonts, &path]
            .iter()
            .for_each(|d| std::fs::create_dir_all(d).unwrap());
        std::fs::write(bin.join("klad"), b"").unwrap();
        std::fs::write(fonts.join(MANIFEST), b"{}").unwrap();
        std::os::unix::fs::symlink(bin.join("klad"), path.join("klad")).unwrap();
        let found = found_from(&path.join("klad"))
            .expect("found through the link")
            .canonicalize()
            .unwrap();
        assert_eq!(found, fonts.canonicalize().unwrap());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_catalogue_lists_the_open_fonts_by_popularity() {
        let found = search("lobster").unwrap();
        assert_eq!(found[0].family, "Lobster");
        assert_eq!(found[0].path, "ofl/lobster");
        assert!(search("pixel").is_ok());
        assert_eq!(listed("Lobster Two Bold").unwrap().family, "Lobster Two");
        assert!(catalogue().unwrap().families.len() > 1500);
    }
    #[test]
    #[ignore = "downloads a family from GitHub"]
    fn a_family_downloads_into_the_user_folder_and_comes_into_a_drawing() {
        let dir = std::env::temp_dir().join(format!("klad-user-fonts-{}", std::process::id()));
        let root = dir.join("art.klad");
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("KLAD_USER_FONTS", dir.join("fonts")) };
        forget();
        let chosen = bring(&root, "Lobster").unwrap();
        assert_eq!(chosen.fetched.as_deref(), Some("Lobster"));
        assert_eq!(chosen.file, "Lobster-Regular.ttf");
        assert!(root.join("fonts/Lobster-Regular.ttf").is_file());
        let family = find("Lobster").unwrap().family;
        assert!(!family.shipped && family.dir.join("OFL.txt").is_file() && family.dir.join(FAMILY).is_file());
        assert_eq!(
            bring(&root, "Lobster").unwrap().fetched,
            None,
            "the second time it is already here"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
