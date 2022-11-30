use std::env::set_var;
use std::ffi::OsStr;
use std::fs;
use std::process::Stdio;
use clap::{arg, ArgAction, command};
// use crate::hello::HelloFS;
use anyhow::{anyhow, Result};
use disk_driver::file::FileDiskDriver;
use execute::Execute;
use fork::{Fork, fork};
use nix::sys::signal;
use retry::delay::Fixed;
use retry::{OperationResult, retry_with_index};
use log::*;
use rfs::{DEVICE_FILE, FORCE_FORMAT, MKFS_FORMAT, MOUNT_POINT, RFS};
use crate::rfs_lib::utils::init_logs;

mod rfs_lib;
mod hello;
// mod utils;

fn main() -> Result<()> {
    let matches = command!() // requires `cargo` feature
        .arg(arg!([mountpoint] "Optional mountpoint to mount on")
            .default_value("tests/mnt"))
        .arg(arg!(-f --front "Keep daemon running in front").action(ArgAction::SetTrue)
            .required(false))
        .arg(arg!(--format "Format disk").action(ArgAction::SetTrue)
            .required(false))
        .arg(arg!(--mkfs "Use mkfs.ext2 to format disk").action(ArgAction::SetTrue)
            .required(false))
        .arg(arg!(-r --read_only "Mount as read only filesystem").action(ArgAction::SetTrue)
            .required(false))
        .arg(arg!(-v --verbose "Print more debug information, or set `RUST_LOG=debug`").action(ArgAction::SetTrue)
            .required(false))
        .arg(
            arg!(-d --device <FILE> "Device path (filesystem storage file)")
                .required(false)
                .default_value("ddriver"),
        )
        .get_matches();

    if matches.get_flag("verbose") {
        set_var("RUST_LOG", "debug");
    }
    init_logs();
    let mountpoint = matches.get_one::<String>("mountpoint").unwrap();
    let device = matches.get_one::<String>("device").unwrap();
    let path_mountpoint = fs::canonicalize(mountpoint)?;
    // let path_device = fs::canonicalize(device)?;
    let abspath_mountpoint = path_mountpoint.to_str().unwrap();
    // let abspath_device = path_device.to_str().unwrap();
    info!("Device: {}", device);
    DEVICE_FILE.set(device.clone()).unwrap();

    MOUNT_POINT.set(abspath_mountpoint.clone().to_string()).unwrap();
    FORCE_FORMAT.set(matches.get_flag("format")).unwrap();
    MKFS_FORMAT.set(matches.get_flag("mkfs")).unwrap();
    // MKFS_FORMAT.set(true).unwrap();

    macro_rules! umount {
        () => {
            {
                use log::*;
                info!("Unmounting {}", MOUNT_POINT.read().unwrap().clone());
                let mut command = execute::command_args!("fusermount", "-u", MOUNT_POINT.read().unwrap().clone());
                command.stdout(Stdio::piped());
                let output = command.execute_output().unwrap();
                info!("fusermount output: {}", String::from_utf8(output.stdout).unwrap());
            }
        };
    }

    pub extern "C" fn signal_handler(_: i32) {
        unsafe { println!("[{}] Received signal and will umount.", libc::getpid()); }
        umount!();
        unsafe { println!("[{}] All Done.", libc::getpid()); }
        std::process::exit(0);
    }

    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(signal_handler),
        signal::SaFlags::SA_NODEFER,
        signal::SigSet::empty(),
    );
    unsafe {
        match signal::sigaction(signal::SIGINT, &sig_action) {
            Ok(_) => {}
            Err(e) => {
                println!("SIGINT signal set failed, {:?}", e);
            }
        }
    }

    let read_only = matches.get_flag("read_only");
    let options = ["-o", if read_only { "ro" } else { "rw" }, "-o", "fsname=rfs"];
    let options = options.iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();
    let retry_times = 3;
    match if matches.get_flag("front") { Ok(Fork::Child) } else { fork() } {
        Ok(Fork::Parent(child)) => {
            info!("Daemon running at pid: {}", child);
            Ok(())
        }
        Ok(Fork::Child) => {
            match retry_with_index(Fixed::from_millis(100), |current_try| {
                info!("[try {}/{}] Mount to {}", current_try, retry_times, abspath_mountpoint);
                let res = fuse::mount(RFS::new(FileDiskDriver::new("")), abspath_mountpoint, &options);
                match res {
                    Ok(_) => {
                        info!("All Done.");
                        OperationResult::Ok(())
                    }
                    Err(e) => {
                        if current_try > retry_times {
                            OperationResult::Err(format!("Failed to mount after {} retries! Err: {}", retry_times, e))
                        } else {
                            umount!();
                            info!("Umount Done.");
                            OperationResult::Retry(format!("Failed to mount, trying to umount..."))
                        }
                    }
                }
            }) {
                Ok(_) => Ok(()),
                Err(e) => Err(anyhow!("Mount failed with {}", e))
            }
        }
        Err(e) => Err(anyhow!("Fork returns error {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fuse::Filesystem;
    // use std::env;
    use std::ffi::OsStr;
    use std::fs;
    use hello::HelloFS;
    use anyhow::Result;

    #[test]
    fn simple_fuse() {
        struct SimpleFuse;
        impl Filesystem for SimpleFuse {}
    }

    #[test]
    fn test_hello() -> Result<()> {
        env_logger::init();
        // let mountpoint = env::args_os().nth(1).unwrap();
        let path = fs::canonicalize("tests/mnt")?;
        let mountpoint = path.to_str().unwrap();
        // let mountpoint = "/home/chiro/mnt";
        println!("mountpoint is {}", mountpoint);
        let options = ["-o", "ro", "-o", "fsname=hello"]
            .iter()
            .map(|o| o.as_ref())
            .collect::<Vec<&OsStr>>();
        fuse::mount(HelloFS, mountpoint, &options).unwrap();
        Ok(())
    }

    #[test]
    fn run_tests() {
        use std::process::Stdio;
        use execute::Execute;
        use std::env;
        use std::path::Path;
        let test_root = Path::new("./tests");
        assert!(env::set_current_dir(&test_root).is_ok());
        println!("Successfully changed working directory to {}!", test_root.display());
        let mut command = execute::command_args!("./test.sh");
        command.stdout(Stdio::piped());
        let output = command.execute_output().unwrap();
        println!("{}", String::from_utf8(output.stdout).unwrap());
    }
}
