use atty::Stream;
use capsules_lib::{
    Capsule, CliMessage, Env, Error, Exitable, ExitableError, FOOTER_SIZE, ListResp,
    MAGIC_NUMBER_ENCRIPTED, MAGIC_NUMBER_PLAIN, Process, RestartPolicy, RunningProcess, SetError,
    Status, SupervisorResp, Table, decrypt,
};
use clap::{Parser, Subcommand};
use postcard::{from_bytes, to_allocvec};
use rpassword::{prompt_password, read_password_from_bufread};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom};
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use sysinfo::{Pid, System, get_current_pid};
use zip::ZipArchive;

fn get_data() -> Result<Vec<u8>, Error> {
    let exe_path = env::current_exe().map_err(|_| Error::NoData)?;
    let mut file = File::open(exe_path).map_err(|_| Error::NoData)?;
    file.seek(SeekFrom::End(-FOOTER_SIZE))
        .map_err(|_| Error::NoData)?;
    let mut footer_bytes = [0u8; FOOTER_SIZE as usize];
    file.read_exact(&mut footer_bytes)
        .map_err(|_| Error::NoData)?;
    let magic = &footer_bytes[8..16];

    if magic != MAGIC_NUMBER_PLAIN && magic != MAGIC_NUMBER_ENCRIPTED {
        return Err(Error::NoData);
    }

    let len_bytes: [u8; 8] = footer_bytes[0..8].try_into().map_err(|_| Error::NoData)?;
    let data_len = u64::from_le_bytes(len_bytes);
    let data_start_offset = -FOOTER_SIZE - (data_len as i64);
    file.seek(SeekFrom::End(data_start_offset))
        .map_err(|_| Error::NoData)?;
    let mut data = vec![0u8; data_len as usize];
    file.read_exact(&mut data).map_err(|_| Error::NoData)?;

    if magic == MAGIC_NUMBER_PLAIN {
        return Ok(data);
    }
    if data.len() < 28 {
        return Err(Error::InvalidDataFormat);
    }
    let password = env::var("__SUPERVISOR_PASSWORD__").map_err(|_| Error::InvalidPassword)?;

    let salt = &data[0..16];
    let nonce = &data[16..28];
    let ciphertext = &data[28..];
    decrypt(&password, salt, nonce, ciphertext)
}

fn read_password() -> Result<String, Error> {
    if !atty::is(Stream::Stdin) {
        let mut reader = BufReader::new(io::stdin());
        return read_password_from_bufread(&mut reader)
            .map(|e| e.trim().to_string())
            .set_error(Error::InternalError);
    }
    prompt_password("Enter password: ").set_error(Error::InternalError)
}

fn extract_files(mut c: Capsule) -> Result<Capsule, Error> {
    let fs_bytes = match &c.fs {
        Some(bytes) => bytes,
        None => return Ok(c),
    };

    let root = get_capsule_cwd()?;
    let cursor = Cursor::new(fs_bytes);
    let mut zip = ZipArchive::new(cursor).map_err(|_| Error::InternalError)?;

    if let Some(files) = &c.files {
        extract_file_map(&mut zip, &root, files)?;
    }
    if let Some(processes) = &c.processes {
        for (name, process) in processes {
            let cwd = process.cwd.as_ref().unwrap_or(name);
            let path = root.join(cwd);
            fs::create_dir_all(&path)
                .set_error(Error::CouldNotCreatePath(path.display().to_string()))?;
            if let Some(files) = &process.files {
                extract_file_map(&mut zip, Path::new(cwd), files)?;
            }
        }
    }
    c.fs.take();
    Ok(c)
}

fn clear_files(c: &Capsule) -> Result<(), Error> {
    let root = get_capsule_cwd()?;
    fs::remove_dir_all(&root).set_error(Error::InternalError)?;
    if let Some(processes) = &c.processes {
        for (name, process) in processes {
            let cwd = process.cwd.as_ref().unwrap_or(name);
            let path = root.join(cwd);
            fs::remove_dir_all(path).set_error(Error::InternalError)?;
        }
    }
    Ok(())
}

fn extract_file_map(
    zip: &mut ZipArchive<Cursor<&Vec<u8>>>,
    root: &Path,
    files: &HashMap<String, String>,
) -> Result<(), Error> {
    for (zip_name, target_path) in files {
        let mut file = zip
            .by_name(zip_name)
            .map_err(|_| Error::CouldNotFindFile(target_path.to_string()))?;

        let out_path = root.join(target_path);

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| Error::CouldNotFindFile(parent.display().to_string()))?;
        }

        let mut out_file = fs::File::create(&out_path)
            .map_err(|_| Error::CouldNotFindFile(out_path.display().to_string()))?;
        std::io::copy(&mut file, &mut out_file)
            .map_err(|_| Error::CouldNotWriteFile(out_path.display().to_string()))?;
    }
    Ok(())
}

fn get_capsule_cwd() -> Result<PathBuf, Error> {
    Ok(env::current_exe()
        .map_err(|_| Error::InternalError)?
        .parent()
        .ok_or(Error::InternalError)?
        .join(".capsule"))
}

fn get_port_file_path() -> Result<PathBuf, Error> {
    Ok(env::current_exe()
        .map_err(|_| Error::InternalError)?
        .parent()
        .ok_or(Error::InternalError)?
        .join(".capsule/capsule.port"))
}

fn get_port() -> Result<u16, Error> {
    std::fs::read_to_string(get_port_file_path()?)
        .map_err(|_| Error::SupervisorCantBeFound)?
        .trim()
        .parse()
        .map_err(|_| Error::SupervisorCantBeFound)
}

fn deamon_run() -> Result<(), Error> {
    let capsule: Capsule = from_bytes(&get_data()?)
        .map_err(|_| Error::InvalidDataFormat)
        .and_then(extract_files)?;

    let mut table = HashMap::<String, RunningProcess>::new();

    let socket = UdpSocket::bind("127.0.0.1:0").map_err(|_| Error::CouldNotStartUdpServer)?;
    socket
        .set_nonblocking(true)
        .map_err(|_| Error::CouldNotStartUdpServer)?;

    let port = socket
        .local_addr()
        .map_err(|_| Error::CouldNotStartUdpServer)?
        .port();

    let path = get_port_file_path()?;
    let parent_dir = path.parent().ok_or(Error::InternalError)?;
    fs::create_dir_all(parent_dir).set_error(Error::InternalError)?;
    fs::write(path, port.to_string()).set_error(Error::InternalError)?;

    let mut buf = [0u8; 4096];
    fn start_child(
        name: &String,
        proc: &Process,
        parent_env: Option<&Env>,
    ) -> Result<Child, Error> {
        let cwd = proc.cwd.as_ref().unwrap_or(name);
        let mut child = Command::new(&proc.cmd);
        child
            .args(proc.args.clone().unwrap_or_default())
            .current_dir(&cwd)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        if let Some(env) = parent_env {
            child.envs(env);
        }
        if let Some(env) = &proc.env {
            child.envs(env);
        }
        child
            .spawn()
            .set_error(Error::FailedToSpawnProcess(name.to_string()))
    }

    if let Some(processes) = &capsule.processes {
        for (name, proc) in processes {
            let Ok(child) = start_child(&name, &proc, capsule.env.as_ref()) else {
                continue;
            };
            let entry = RunningProcess {
                name: name.clone(),
                status: Status::Running(child.id()),
                config: proc.clone(),
                child,
                started: Instant::now(),
                force_restart: false,
                restarts: 0,
            };
            table.insert(entry.name.clone(), entry);
        }
    }

    let mut s = System::new();
    let mut pids = table
        .iter()
        .map(|(_, p)| Pid::from_u32(p.child.id()))
        .collect::<Vec<_>>();

    let pid = get_current_pid().map(|p| vec![p]).unwrap_or_default();
    pids.append(&mut pid.clone());
    let mut last_refresh = Instant::now();

    loop {
        if let Ok((len, client_addr)) = socket.recv_from(&mut buf) {
            if let Ok(msg) = from_bytes::<CliMessage>(&buf[..len]) {
                match msg {
                    CliMessage::Kill { name } => {
                        if let Some(entry) = table.get_mut(&name) {
                            match entry.child.kill() {
                                Ok(_) => {
                                    entry.status = Status::Killed;
                                    entry.child.try_wait().ok();
                                    to_allocvec(&SupervisorResp::Ok)
                                }
                                Err(_) => to_allocvec(&SupervisorResp::Error(Error::InternalError)),
                            }
                        } else {
                            to_allocvec(&SupervisorResp::Error(Error::ProcessNotFound(name)))
                        }
                        .ok()
                        .map(|resp| socket.send_to(&resp, client_addr).ok())
                        .log(Error::InternalError);
                    }
                    CliMessage::Restart { name } => {
                        if let Some(entry) = table.get_mut(&name) {
                            match entry.child.kill() {
                                Ok(_) => {
                                    entry.status = Status::Starting;
                                    entry.force_restart = true;
                                    entry.child.try_wait().ok();
                                    to_allocvec(&SupervisorResp::Ok)
                                }
                                Err(_) => to_allocvec(&SupervisorResp::Error(Error::InternalError)),
                            }
                        } else {
                            to_allocvec(&SupervisorResp::Error(Error::ProcessNotFound(name)))
                        }
                        .ok()
                        .map(|resp| socket.send_to(&resp, client_addr).ok())
                        .log(Error::InternalError);
                    }
                    CliMessage::List => {
                        let table = table
                            .iter()
                            .map(|(_, p)| {
                                let (cpu_usage, memory_usage, run_time, disk_usage) = s
                                    .process(sysinfo::Pid::from_u32(p.child.id()))
                                    .map(|i| {
                                        let disk = i.disk_usage();
                                        (
                                            i.cpu_usage(),
                                            i.memory(),
                                            i.run_time(),
                                            (disk.total_read_bytes, disk.total_written_bytes),
                                        )
                                    })
                                    .unwrap_or_default();
                                ListResp {
                                    status: p.status,
                                    name: p.name.clone(),
                                    cpu_usage,
                                    memory_usage,
                                    disk_usage,
                                    run_time,
                                    restarts: p.restarts,
                                }
                            })
                            .collect();
                        let resp = SupervisorResp::List(table);
                        to_allocvec(&resp)
                            .map(|resp| socket.send_to(&resp, client_addr))
                            .set_error(Error::InternalError)
                            .log();
                    }
                    CliMessage::KillAll => {
                        for (_, proc) in table.iter_mut() {
                            match proc.child.kill() {
                                Ok(_) => {
                                    proc.status = Status::Killed;
                                    proc.child.try_wait().ok();
                                }
                                Err(_) => {}
                            };
                        }
                        to_allocvec(&SupervisorResp::Ok)
                            .map(|resp| socket.send_to(&resp, client_addr))
                            .set_error(Error::InternalError)
                            .log();
                    }
                    CliMessage::TareDown => {
                        for (_, proc) in table.iter_mut() {
                            proc.child.kill().ok();
                            proc.child.try_wait().ok();
                        }
                        let resp = match clear_files(&capsule) {
                            Ok(_) => SupervisorResp::Ok,
                            Err(_) => SupervisorResp::Error(Error::InternalError), // todo return proper error
                        };
                        to_allocvec(&resp)
                            .map(|resp| socket.send_to(&resp, client_addr))
                            .set_error(Error::InternalError)
                            .log();
                        return Ok(());
                    }
                    CliMessage::Status => {
                        to_allocvec(&SupervisorResp::Version(capsule.version.clone()))
                            .map(|resp| socket.send_to(&resp, client_addr))
                            .set_error(Error::InternalError)
                            .log();
                    }
                    CliMessage::KillDeamon => {
                        to_allocvec(&SupervisorResp::Ok)
                            .map(|resp| socket.send_to(&resp, client_addr))
                            .set_error(Error::InternalError)
                            .log();
                        return Ok(());
                    }
                }
            }
        }

        for (_, proc) in table.iter_mut() {
            if proc.status == Status::Killed {
                continue;
            }
            let next_run = Duration::from_millis(proc.config.restart_delay.unwrap_or(10));
            if proc.started.elapsed() < next_run {
                continue;
            }
            match proc.child.try_wait() {
                Ok(Some(status)) => {
                    let (should_restart, inc) = if proc.force_restart {
                        proc.force_restart = false;
                        (true, 0)
                    } else {
                        let restart = proc
                            .config
                            .restart_policy
                            .as_ref()
                            .unwrap_or(&RestartPolicy::Never);
                        (
                            (!status.success() && restart == &RestartPolicy::OnFailure)
                                || (restart == &RestartPolicy::Always),
                            1,
                        )
                    };
                    if should_restart {
                        start_child(&proc.name, &proc.config, capsule.env.as_ref())
                            .map(|child| {
                                proc.status = Status::Running(child.id());
                                proc.child = child;
                                proc.restarts += inc;
                                proc.started = Instant::now();
                            })
                            .ok();
                    } else {
                        proc.status = Status::Exited(status.code().unwrap_or(-9999))
                    }
                }
                _ => continue,
            };
        }

        if last_refresh.elapsed() > sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
            pids = table
                .iter()
                .map(|(_, p)| sysinfo::Pid::from_u32(p.child.id()))
                .collect::<Vec<_>>();
            pids.append(&mut pid.clone());
            s.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::Some(&pids),
                true,
                sysinfo::ProcessRefreshKind::everything(),
            );
            last_refresh = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn cli_deamon_start() -> Result<(), Error> {
    if cli_deamon_status().is_ok() {
        return Ok(());
    }
    let exe_path = env::current_exe().set_error(Error::InternalError)?;
    let mut file =
        File::open(&exe_path).set_error(Error::CouldNotReadFile(exe_path.display().to_string()))?;
    file.seek(SeekFrom::End(-FOOTER_SIZE))
        .set_error(Error::InternalError)?;
    let mut footer_bytes = [0u8; FOOTER_SIZE as usize];
    file.read_exact(&mut footer_bytes)
        .set_error(Error::InternalError)?;
    let magic = &footer_bytes[8..16];
    // could be an attack vector
    // if current_exe is swaped after the password read
    // maybe include a checksum or something
    let mut cmd = Command::new(exe_path);
    cmd.arg("supervisor");
    if magic == MAGIC_NUMBER_ENCRIPTED {
        cmd.env("__SUPERVISOR_PASSWORD__", read_password()?);
    }
    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .set_error(Error::InternalError)?;
    Ok(())
}

fn get_socket() -> Result<(UdpSocket, u16), Error> {
    let port = get_port()?;
    let socket = UdpSocket::bind("127.0.0.1:0").set_error(Error::CouldNotStartUdpServer)?;
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .set_error(Error::InternalError)?;
    Ok((socket, port))
}

fn send_cli_cmd(
    req: CliMessage,
    cb: impl Fn(SupervisorResp) -> Result<(), Error>,
) -> Result<(), Error> {
    let (socket, port) = get_socket()?;
    let data = to_allocvec(&req).set_error(Error::InternalError)?;
    socket
        .send_to(&data, ("127.0.0.1", port))
        .set_error(Error::SupervisorCantBeFound)?;
    let mut buf = [0u8; 4096];
    let len = socket
        .recv(&mut buf)
        .set_error(Error::SupervisorCantBeFound)?;
    let resp: SupervisorResp = from_bytes(&buf[..len]).set_error(Error::InternalError)?;
    if let SupervisorResp::Error(e) = &resp {
        return Err(e.clone());
    }
    cb(resp)
}

fn cli_proc_list() -> Result<(), Error> {
    send_cli_cmd(CliMessage::List, |resp| {
        match resp {
            SupervisorResp::List(processes) => {
                println!("{}", Table::from(processes));
            }
            _ => {}
        }
        return Ok(());
    })
}

fn cli_proc_kill(name: String) -> Result<(), Error> {
    send_cli_cmd(CliMessage::Kill { name: name.clone() }, |_| Ok(()))
        .set_error(Error::CouldNotKillProcess(name.clone()))?;
    println!("Process {} killed!", name);
    return Ok(());
}

fn cli_proc_restart(name: String) -> Result<(), Error> {
    send_cli_cmd(CliMessage::Restart { name: name.clone() }, |_| Ok(()))
        .set_error(Error::CouldNotKillProcess(name.clone()))?;
    println!("Process {} restarting!", name);
    return Ok(());
}

fn cli_proc_kill_all() -> Result<(), Error> {
    send_cli_cmd(CliMessage::KillAll, |_| Ok(()))?;
    println!("Ok!");
    return Ok(());
}

fn cli_deamon_tare_down() -> Result<(), Error> {
    send_cli_cmd(CliMessage::TareDown, |_| Ok(()))?;
    println!("Ok!");
    return Ok(());
}

fn cli_deamon_kill() -> Result<(), Error> {
    send_cli_cmd(CliMessage::TareDown, |_| Ok(()))?;
    println!("Ok!");
    return Ok(());
}

fn cli_deamon_status() -> Result<(), Error> {
    send_cli_cmd(CliMessage::Status, |resp| match resp {
        SupervisorResp::Version(v) => {
            println!("Status.        : Ok");
            println!("Capsule version: {}", v);
            println!("Deamon  version: {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(Error::InternalError),
    })
}

fn cli_deamon_version() -> Result<(), Error> {
    match send_cli_cmd(CliMessage::Status, |resp| match resp {
        SupervisorResp::Version(v) => {
            println!("Capsule version: {}", v);
            println!("Deamon version: {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(Error::InternalError),
    }) {
        Ok(v) => Ok(v),
        Err(_) => {
            println!("Deamon version: {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

#[derive(Debug, Subcommand)]
enum Deamon {
    /// Starts the supervisor
    Start,
    /// Warning! this will remove all files, and stop all procesess and the supervisor
    TareDown,
    /// Returns the status
    Status,
    /// Kills the suppervisor, use proc kill to kill a specific process
    Kill,
}

#[derive(Debug, Subcommand)]
enum Proc {
    /// Kills a process
    Kill { name: String },
    /// Restart a process
    Restart { name: String },
    /// Kills all a processes keeps the suppervisor running
    KillAll,
    /// Lists data about all the processes
    List,
}

#[derive(Parser, Debug)]
#[command(disable_version_flag = true, about, long_about = None)]
enum Args {
    #[command(subcommand)]
    Deamon(Deamon),

    #[command(subcommand)]
    Proc(Proc),

    #[clap(hide = true)]
    Supervisor,

    #[clap(about = "Print version")]
    Version,
}

fn main() {
    let args = Args::parse();

    match args {
        Args::Deamon(demon) => match demon {
            Deamon::Start => cli_deamon_start(),
            Deamon::TareDown => cli_deamon_tare_down(),
            Deamon::Kill => cli_deamon_kill(),
            Deamon::Status => cli_deamon_status(),
        },
        Args::Proc(proc) => match proc {
            Proc::Kill { name } => cli_proc_kill(name),
            Proc::Restart { name } => cli_proc_restart(name),
            Proc::KillAll => cli_proc_kill_all(),
            Proc::List => cli_proc_list(),
        },
        Args::Supervisor => deamon_run(),
        Args::Version => cli_deamon_version(),
    }
    .exit();
}
