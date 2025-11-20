use bytesize::ByteSize;
use humanize_duration::prelude::DurationExt;
use semver::Version;
use std::collections::HashMap;
use std::fmt::Display;

use std::process::{self, Child};
use std::time::{Duration, Instant};
use thiserror::Error;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub const RUNTIME_TARGETS: &[(&str, &str)] = &[
    // WINDOWS
    ("x86_64-pc-windows-gnu", ".exe"),
    // LINUX
    ("x86_64-unknown-linux-musl", ""),
    ("x86_64-unknown-linux-gnu", ""),
    ("aarch64-unknown-linux-gnu", ""),
    ("aarch64-unknown-linux-musl", ""),
    ("armv7-unknown-linux-gnueabihf", ""),
    ("armv7-unknown-linux-musleabihf", ""),
    // APPLE
    ("aarch64-apple-darwin", ""),
    ("x86_64-apple-darwin", ""),
];

pub const MAGIC_NUMBER_PLAIN: &[u8; 8] = b"SETENV_P";
pub const MAGIC_NUMBER_ENCRIPTED: &[u8; 8] = b"SETENV_E";
pub const FOOTER_SIZE: i64 = 16;

fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, 600_000, &mut key);
    key
}

pub fn encrypt(password: &str, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let mut salt = vec![0u8; 16];
    rand::rng().fill_bytes(&mut salt);

    let key_bytes = derive_key(password, &salt);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let mut nonce_bytes = vec![0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| e.to_string())?;

    Ok((salt, nonce_bytes, ciphertext))
}

pub fn decrypt(
    password: &str,
    salt: &[u8],
    nonce_bytes: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, Error> {
    let key_bytes = derive_key(password, salt);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| Error::InvalidPassword)
}

pub type Env = HashMap<String, String>;

#[cfg_attr(test, derive(schemars::JsonSchema))]
#[derive(Serialize, Deserialize)]
pub struct Capsule {
    /// The version of the capsule
    pub version: Version,
    /// Global environment varriables
    pub env: Option<Env>,
    #[cfg_attr(test, schemars(skip))]
    pub fs: Option<Vec<u8>>,
    /// Global files
    /// source -> target
    pub files: Option<HashMap<String, String>>,
    /// Processes to spawn
    pub processes: Option<HashMap<String, Process>>,
}

#[cfg_attr(test, derive(schemars::JsonSchema))]
#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    Never,
    Always,
    OnFailure,
}

#[cfg_attr(test, derive(schemars::JsonSchema))]
#[derive(Serialize, Deserialize, Clone)]
pub struct Process {
    /// Command to execute
    pub cmd: String,
    /// Command arguments
    pub args: Option<Vec<String>>,
    /// Process working directory
    pub cwd: Option<String>,
    /// Env vars
    pub env: Option<Env>,
    /// Restart policy
    pub restart_policy: Option<RestartPolicy>,
    /// Time in ms to wait before restarting the process
    pub restart_delay: Option<u64>,
    /// Files to embed
    /// source -> target
    pub files: Option<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Starting,
    /// pid
    Running(u32),
    // Exit Code
    Exited(i32),
    Killed,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Starting => write!(f, "starting"),
            Status::Running(pid) => write!(f, "Running pid {}", pid),
            Status::Exited(code) => write!(f, "Exited code {}", code),
            Status::Killed => write!(f, "Killed"),
        }
    }
}

pub struct RunningProcess {
    pub name: String,
    pub status: Status,
    pub config: Process,
    pub child: Child,
    pub started: Instant,
    pub force_restart: bool,
    pub restarts: u32,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum CliMessage {
    Kill { name: String },
    Restart { name: String },
    List,
    Stop,
    KillAll,
    Status,
}

#[derive(Serialize, Deserialize)]
pub enum SupervisorResp {
    Ok,
    Error(Error),
    List(Vec<ListResp>),
    Version(Version),
}

#[derive(Serialize, Deserialize)]
pub struct ListResp {
    pub status: Status,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub disk_usage: (u64, u64),
    pub restarts: u32,
    pub run_time: u64,
}

#[derive(Error, Debug, Serialize, Deserialize, Clone)]
pub enum Error {
    #[error("Process {0:?} not found")]
    ProcessNotFound(String),
    #[error("Supervisor cant be found")]
    SupervisorCantBeFound,
    #[error("Could not start Udp server")]
    CouldNotStartUdpServer,
    #[error("No data provided")]
    NoData,
    #[error("Invalid password")]
    InvalidPassword,
    #[error("Invalid data format")]
    InvalidDataFormat,
    #[error("Could not find file {0:?}")]
    CouldNotFindFile(String),

    #[error("Could not read file {0:?}")]
    CouldNotReadFile(String),

    #[error("Could not create {0:?}")]
    CouldNotCreatePath(String),

    #[error("Internal Error")]
    InternalError,

    #[error("Could not write file {0:?}")]
    CouldNotWriteFile(String),

    #[error("Could not kill process {0:?}")]
    CouldNotKillProcess(String),

    #[error("Failed to spawn process {0:?}")]
    FailedToSpawnProcess(String),

    #[error("Could not encrypt file")]
    CouldNotEncryptFile,

    #[error("Unsupported target")]
    UnsupportedTarget(String),
}

impl<T> Exitable<T> for Result<T, Error> {
    fn exit(self) -> T {
        match self {
            Ok(v) => v,
            Err(e) => e.exit(),
        }
    }

    fn log(self) -> () {
        match self {
            Ok(_) => (),
            Err(e) => e.log(),
        }
    }
}

impl<T> ExitableError<T> for Option<T> {
    fn exit(self, e: Error) -> T {
        match self {
            Some(t) => t,
            None => e.exit(),
        }
    }

    fn log(self, e: Error) -> () {
        match self {
            Some(_) => (),
            None => e.log(),
        }
    }
}

pub trait Exitable<T> {
    fn exit(self) -> T;
    fn log(self) -> ();
}

impl<T, E> SetError<T> for Result<T, E> {
    fn set_error(self, e: Error) -> Result<T, Error> {
        self.map_err(|_| e)
    }
}

pub trait SetError<T> {
    fn set_error(self, e: Error) -> Result<T, Error>;
}

pub trait ExitableError<T> {
    fn exit(self, e: Error) -> T;
    fn log(self, e: Error) -> ();
}

impl Error {
    pub fn exit(self) -> ! {
        eprintln!("Error: {}", self.to_string());
        process::exit(1);
    }
    pub fn log(&self) {
        eprintln!("Error: {}", self.to_string())
    }
}

pub struct Table(pub Vec<ListResp>);

impl From<Vec<ListResp>> for Table {
    fn from(value: Vec<ListResp>) -> Self {
        Table(value)
    }
}

impl Display for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let head = vec![
            "Name".to_string(),
            "Status".to_string(),
            "CPU usage".to_string(),
            "Memory usage".to_string(),
            "IO Reads".to_string(),
            "IO Writes".to_string(),
            "Run time".to_string(),
            "Restarts".to_string(),
        ];
        let mut max_size: Vec<_> = head.iter().map(|s| s.len()).collect();

        let mut lines: Vec<_> = vec![head];
        self.0.iter().for_each(|d| {
            let name = d.name.to_string();
            let cpu_usage = d.cpu_usage;
            let status = d.status;
            let duration = Duration::from_secs(d.run_time);
            let runtime = duration.human(humanize_duration::Truncate::Second);
            let memory_usage = ByteSize::b(d.memory_usage);
            let disk_read = ByteSize::b(d.disk_usage.0);
            let disk_write = ByteSize::b(d.disk_usage.1);
            let restarts = d.restarts;
            let line = vec![
                format!("{}", name),
                format!("{}", status),
                format!("{:.2}%", cpu_usage),
                format!("{}", memory_usage),
                format!("{}", disk_read),
                format!("{}", disk_write),
                format!("{}", runtime),
                format!("{}", restarts),
            ];
            for (i, c) in line.iter().enumerate() {
                max_size[i] = max_size[i].max(c.len())
            }
            lines.push(line);
        });

        let top = {
            let mut s = String::from("┌");
            for (i, w) in max_size.iter().enumerate() {
                s.push_str(&"─".repeat(*w + 2));
                s.push(if i + 1 == max_size.len() {
                    '┐'
                } else {
                    '┬'
                });
            }
            s
        };

        let header_sep = {
            let mut s = String::from("├");
            for (i, w) in max_size.iter().enumerate() {
                s.push_str(&"─".repeat(*w + 2));
                s.push(if i + 1 == max_size.len() {
                    '┤'
                } else {
                    '┼'
                });
            }
            s
        };

        let bottom = {
            let mut s = String::from("└");
            for (i, w) in max_size.iter().enumerate() {
                s.push_str(&"─".repeat(*w + 2));
                s.push(if i + 1 == max_size.len() {
                    '┘'
                } else {
                    '┴'
                });
            }
            s
        };

        // Print top border
        writeln!(f, "{}", top)?;

        for (row_i, row) in lines.iter().enumerate() {
            write!(f, "│")?;
            for (i, col) in row.iter().enumerate() {
                write!(f, " {:^width$} │", col, width = max_size[i])?;
            }
            writeln!(f)?;

            if row_i == 0 {
                // Header separator
                writeln!(f, "{}", header_sep)?;
            }
        }

        // Bottom border
        writeln!(f, "{}", bottom)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::Capsule;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    const SCHEMAS_FOLDER: &str = "shcemas";
    const SCHEMA_NAME: &str = "capsule.json";

    #[test]
    fn gen_schema() {
        let schema = schemars::schema_for!(Capsule);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
            .parent()
            .unwrap()
            .join(SCHEMAS_FOLDER);
        fs::write(manifest_dir.join(SCHEMA_NAME), json).unwrap();
    }
}
