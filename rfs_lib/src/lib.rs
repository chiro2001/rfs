#[cxx::bridge]
mod ffi {
    extern "Rust" {
        fn add(left: usize, right: usize) -> usize;
    }
}

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

mod hello;

#[cfg(test)]
mod tests {
    use super::*;
    use fuse::Filesystem;
    // use std::env;
    use std::ffi::OsStr;
    use hello::HelloFS;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }

    #[test]
    fn simple_fuse() {
        struct SimpleFuse;
        impl Filesystem for SimpleFuse {}
    }

    #[test]
    fn test_hello() {
        env_logger::init();
        // let mountpoint = env::args_os().nth(1).unwrap();
        let mountpoint = "/home/chiro/mnt";
        let options = ["-o", "ro", "-o", "fsname=hello"]
            .iter()
            .map(|o| o.as_ref())
            .collect::<Vec<&OsStr>>();
        fuse::mount(HelloFS, &mountpoint, &options).unwrap();
    }
}
