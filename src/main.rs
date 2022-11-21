use std::ffi::OsStr;
use std::fs;
use clap::{arg, ArgAction, command};
use crate::hello::HelloFS;
use anyhow::{anyhow, Result};
use fork::{Fork, fork};

mod lib;
mod hello;

fn main() -> Result<()> {
    let matches = command!() // requires `cargo` feature
        .arg(arg!([mountpoint] "Optional mountpoint to mount on")
            .default_value("tests/mnt"))
        .arg(arg!(-f --front "Keep daemon running in front").action(ArgAction::SetTrue)
            .required(false))
        .arg(
            arg!(--device <FILE> "Device path (filesystem storage file)")
                .required(false)
                .default_value("ddriver"),
        )
        .get_matches();

    env_logger::init();
    let mountpoint = matches.get_one::<String>("mountpoint").unwrap();
    let device = matches.get_one::<String>("device").unwrap();
    let path_mountpoint = fs::canonicalize(mountpoint)?;
    // let path_device = fs::canonicalize(device)?;
    let abspath_mountpoint = path_mountpoint.to_str().unwrap();
    // let abspath_device = path_device.to_str().unwrap();
    println!("Device: {}", device);
    println!("Mounting to {}", abspath_mountpoint);
    let options = ["-o", "ro", "-o", "fsname=rfs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();
    match if matches.get_flag("front") { Ok(Fork::Child) } else { fork() } {
        Ok(Fork::Parent(child)) => {
            println!("Daemon running at pid: {}", child);
            Ok(())
        }
        Ok(Fork::Child) => {
            // fuse::mount(lib::RFS, &mountpoint, &options).unwrap();
            fuse::mount(HelloFS, abspath_mountpoint, &options).unwrap();
            println!("All Done.");
            Ok(())
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
