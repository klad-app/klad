#![doc = include_str!("../README.md")]

pub use serde_json::{self, Value, json};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug)]
pub enum Error {
    Io {
        command: String,
        source: std::io::Error,
    },
    Failed {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    Json {
        command: String,
        source: serde_json::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { command, source } => write!(f, "{command}: {source}"),
            Error::Failed { command, stderr, .. } => write!(f, "{command}: {}", stderr.trim()),
            Error::Json { command, source } => write!(f, "{command} printed JSON that does not parse: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            Error::Json { source, .. } => Some(source),
            Error::Failed { .. } => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

const NONE: [&str; 0] = [];

#[derive(Clone, Debug)]
pub struct Klad {
    program: PathBuf,
}

impl Default for Klad {
    fn default() -> Self {
        Self::new()
    }
}

impl Klad {
    pub fn new() -> Self {
        Self::at(std::env::var_os("KLAD").map_or_else(|| PathBuf::from("klad"), PathBuf::from))
    }

    pub fn at(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn version(&self) -> Result<String> {
        Ok(self.run(["--version"])?.trim().to_string())
    }

    pub fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.exec(args, None)
    }

    pub fn json<I, S>(&self, args: I) -> Result<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = owned(args);
        let text = self.exec(&args, None)?;
        serde_json::from_str(&text).map_err(|source| Error::Json {
            command: self.describe(&args),
            source,
        })
    }

    pub fn drawing(&self, path: impl Into<PathBuf>) -> Drawing {
        Drawing {
            klad: self.clone(),
            path: path.into(),
        }
    }

    fn exec<I, S>(&self, args: I, input: Option<&[u8]>) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = owned(args);
        let command = self.describe(&args);
        let io = |source| Error::Io {
            command: command.clone(),
            source,
        };
        let mut child = Command::new(&self.program)
            .args(&args)
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(io)?;
        let stdin = child.stdin.take();
        let (out, written) = std::thread::scope(|scope| {
            let writer = scope.spawn(move || match (stdin, input) {
                (Some(mut pipe), Some(bytes)) => pipe.write_all(bytes),
                _ => Ok(()),
            });
            (child.wait_with_output(), writer.join().unwrap_or(Ok(())))
        });
        let out = out.map_err(io)?;
        if !out.status.success() {
            return Err(Error::Failed {
                command,
                status: out.status.code(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        written.map_err(io)?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn describe(&self, args: &[OsString]) -> String {
        std::iter::once(self.program.as_os_str())
            .chain(args.iter().map(OsString::as_os_str))
            .map(OsStr::to_string_lossy)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Clone, Debug)]
pub struct Drawing {
    klad: Klad,
    path: PathBuf,
}

impl Drawing {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn create(&self, width: u32, height: u32, background: Option<&str>) -> Result<String> {
        let (width, height) = (width.to_string(), height.to_string());
        let mut args = vec!["--width", &width, "--height", &height];
        args.extend(background.map(|b| ["--background", b]).into_iter().flatten());
        self.command("new", args)
    }

    pub fn command<I, S>(&self, name: &str, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.klad.run(self.args(name, args))
    }

    pub fn json<I, S>(&self, name: &str, args: I) -> Result<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.klad.json(self.args(name, args))
    }

    pub fn apply<I>(&self, ops: I) -> Result<Vec<String>>
    where
        I: IntoIterator<Item = Value>,
    {
        let lines: String = ops.into_iter().map(|op| format!("{op}\n")).collect();
        let out = self.klad.exec(self.args("apply", NONE), Some(lines.as_bytes()))?;
        Ok(out.lines().map(str::to_string).collect())
    }

    pub fn document(&self) -> Result<Value> {
        let command = format!("reading {}", self.path.join("doc.json").display());
        let bytes = std::fs::read(self.path.join("doc.json")).map_err(|source| Error::Io {
            command: command.clone(),
            source,
        })?;
        serde_json::from_slice(&bytes).map_err(|source| Error::Json { command, source })
    }

    pub fn ls(&self, frame: u32) -> Result<Value> {
        self.json("ls", ["--json", "--frame", &frame.to_string()])
    }

    pub fn show(&self, id: &str) -> Result<Value> {
        self.json("show", [id])
    }

    pub fn strokes(&self, layer: &str, frame: u32) -> Result<Value> {
        self.json("strokes", [layer, "--json", "--frame", &frame.to_string()])
    }

    pub fn notes(&self) -> Result<Value> {
        self.json("notes", ["--json"])
    }

    pub fn selection(&self) -> Result<Value> {
        self.json("selection", ["--json"])
    }

    pub fn render(&self, out: &Path, frame: u32, scale: f32) -> Result<PathBuf> {
        let args = [
            OsString::from("--frame"),
            frame.to_string().into(),
            "--scale".into(),
            scale.to_string().into(),
            "--out".into(),
            out.as_os_str().to_os_string(),
        ];
        self.command("render", args)?;
        Ok(out.to_path_buf())
    }

    pub fn undo(&self) -> Result<String> {
        self.command("undo", NONE)
    }

    pub fn redo(&self) -> Result<String> {
        self.command("redo", NONE)
    }

    fn args<I, S>(&self, name: &str, args: I) -> Vec<OsString>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        [OsString::from(name), self.path.as_os_str().to_os_string()]
            .into_iter()
            .chain(owned(args))
            .collect()
    }
}

fn owned<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter().map(|a| a.as_ref().to_os_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_is_an_io_error_naming_the_command() {
        let klad = Klad::at("/no/such/klad");
        let error = klad.drawing("a.klad").ls(0).unwrap_err();
        assert!(matches!(error, Error::Io { .. }));
        assert!(
            error
                .to_string()
                .starts_with("/no/such/klad ls a.klad --json --frame 0")
        );
    }

    #[test]
    fn klad_on_the_path_unless_the_environment_names_one() {
        let expected = std::env::var_os("KLAD").map_or_else(|| PathBuf::from("klad"), PathBuf::from);
        assert_eq!(Klad::new().program(), expected);
        assert_eq!(Klad::at("/opt/klad/klad").program(), Path::new("/opt/klad/klad"));
    }
}
