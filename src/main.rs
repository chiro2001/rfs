use std::ffi::OsStr;

mod lib;
mod hello;

fn main() {
    println!("mounting...");
    let mountpoint = "/home/chiro/mnt";
    let options = ["-o", "ro", "-o", "fsname=rfs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();
    fuse::mount(lib::RFS, &mountpoint, &options).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use fuse::Filesystem;
    // use std::env;
    use std::ffi::OsStr;
    use hello::HelloFS;

    #[test]
    fn simple_fuse() {
        struct SimpleFuse;
        impl Filesystem for SimpleFuse {}
    }

    #[test]
    fn test_hello() {
        env_logger::init();
        // let mountpoint = env::args_os().nth(1).unwrap();
        let mountpoint = "tests/mnt";
        let options = ["-o", "ro", "-o", "fsname=hello"]
            .iter()
            .map(|o| o.as_ref())
            .collect::<Vec<&OsStr>>();
        fuse::mount(HelloFS, &mountpoint, &options).unwrap();
    }
}
