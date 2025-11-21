mod runtime_binaries {
    include!(concat!(env!("OUT_DIR"), "/runtime_binaries.rs"));
}

use capsules_lib::{
    ASCII_ART, Capsule, Error, MAGIC_NUMBER_ENCRYPTED, MAGIC_NUMBER_PLAIN, RUNTIME_TARGETS,
    SetError, encrypt,
};
use clap::{Parser, builder::PossibleValuesParser};
use runtime_binaries::RUNTIME_BINARIES;
use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

fn target_parser() -> PossibleValuesParser {
    PossibleValuesParser::new(RUNTIME_BINARIES.iter().map(|(k, _)| *k))
}

/// The Capsules compiler
#[derive(Parser, Debug)]
#[command(version, about, long_about = None, before_help=ASCII_ART)]
struct Args {
    /// The capsule input file
    #[arg(short, long)]
    input_file: PathBuf,

    /// Output target
    #[arg(short, long, value_parser=target_parser())]
    target: String,

    /// Encryption password
    #[arg(short, long)]
    password: Option<String>,

    /// Output executable
    #[arg(short, long)]
    output_path: Option<PathBuf>,
}

fn main() {
    match run() {
        Ok(()) => (),
        Err(e) => e.exit(),
    }
}

fn run() -> Result<(), Error> {
    let args = Args::parse();

    let cwd = env::current_dir().set_error(Error::InternalError)?;
    let input_path = if !args.input_file.is_absolute() {
        cwd.join(args.input_file)
    } else {
        args.input_file
    };
    let target = args.target;
    let output_path = args
        .output_path
        .unwrap_or_else(|| default_output(&input_path, &target));
    let runtime =
        runtime_for_target(&target).ok_or(Error::UnsupportedTarget(target.to_string()))?;
    let input_file_content = fs::read_to_string(&input_path)
        .set_error(Error::CouldNotReadFile(input_path.display().to_string()))?;

    let file = deserialize(&input_file_content).ok_or(Error::InvalidDataFormat)?;

    let base = input_path
        .parent()
        .ok_or(Error::CouldNotReadFile(input_path.display().to_string()))?;

    let input_bytes = to_binary(file, base).ok_or(Error::InvalidDataFormat)?;

    let mut file = File::create(&output_path)
        .set_error(Error::CouldNotWriteFile(output_path.display().to_string()))?;

    let (input_bytes, magic_number) = if let Some(password) = args.password {
        let (mut salt, mut nonce_bytes, mut ciphertext) =
            encrypt(&password, &input_bytes).set_error(Error::CouldNotEncryptFile)?;
        salt.append(&mut nonce_bytes);
        salt.append(&mut ciphertext);
        (salt, MAGIC_NUMBER_ENCRYPTED)
    } else {
        (input_bytes, MAGIC_NUMBER_PLAIN)
    };

    (|| {
        file.write_all(runtime)?;
        file.write_all(&input_bytes)?;
        file.write_all(&(input_bytes.len() as u64).to_le_bytes())?;
        file.write_all(magic_number)?;
        Ok::<_, std::io::Error>(())
    })()
    .set_error(Error::InternalError)?;

    make_executable(&output_path).ok_or(Error::InternalError)?;
    Ok(())
}

fn default_output(input: &Path, target: &str) -> PathBuf {
    let extension = RUNTIME_TARGETS
        .iter()
        .find(|(triple, _)| *triple == target)
        .map(|(_, ext)| *ext)
        .unwrap_or("");

    let stem = input
        .file_stem()
        .or_else(|| input.file_name())
        .unwrap_or_else(|| input.as_os_str());
    let parent = input.parent().filter(|p| !p.as_os_str().is_empty());
    let output_name = format!("{}-{target}{}", stem.to_string_lossy(), extension);
    match parent {
        Some(dir) => dir.join(output_name),
        None => PathBuf::from(output_name),
    }
}

fn runtime_for_target(target: &str) -> Option<&'static [u8]> {
    RUNTIME_BINARIES
        .iter()
        .find(|(triple, _)| *triple == target)
        .map(|(_, bytes)| *bytes)
}

fn make_executable(path: &Path) -> Option<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path).ok()?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).ok()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Some(())
    }
}

fn deserialize(file_data: &str) -> Option<Capsule> {
    let json: Result<Capsule, _> = serde_json::from_str(file_data);
    match json {
        Ok(json) => return Some(json),
        Err(err) => {
            if !err.is_syntax() {
                return None;
            }
        }
    }
    let toml: Result<Capsule, _> = toml::from_str(file_data);
    if toml.is_ok() {
        return Some(toml.unwrap());
    }
    return None;
}

fn to_binary(mut c: Capsule, base: &Path) -> Option<Vec<u8>> {
    let buff = std::io::Cursor::new(Vec::new());
    let mut zip_file = ZipWriter::new(buff);
    let mut new_mapping = HashMap::new();
    if let Some(files) = &c.files {
        write_files(&mut zip_file, &mut new_mapping, files, base)?;
        c.files = Some(new_mapping);
    }
    if let Some(processes) = &mut c.processes {
        for process in processes.values_mut() {
            if let Some(files) = &process.files {
                let mut new_mapping = HashMap::new();
                write_files(&mut zip_file, &mut new_mapping, files, base)?;
                process.files = Some(new_mapping);
            }
        }
    }
    let writer = zip_file.finish().ok()?;
    let zip_bytes = writer.into_inner();
    c.fs = Some(zip_bytes);
    postcard::to_allocvec(&c).ok()
}

fn write_files(
    zip_file: &mut ZipWriter<std::io::Cursor<Vec<u8>>>,
    new_mapping: &mut HashMap<String, String>,
    files: &HashMap<String, String>,
    cwd: &Path,
) -> Option<()> {
    Some(for (local_path, target) in files {
        let local_path = PathBuf::from(local_path);
        let local_path = if local_path.is_absolute() {
            local_path
        } else {
            cwd.join(local_path)
        };
        let bytes = fs::read(local_path).ok()?;
        let random_name = Uuid::new_v4().to_string();
        zip_file
            .start_file(&random_name, SimpleFileOptions::default())
            .ok()?;
        zip_file.write_all(&bytes).ok()?;
        new_mapping.insert(random_name, target.to_string());
    })
}
