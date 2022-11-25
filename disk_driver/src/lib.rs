pub trait DiskDriver {
    fn ddriver_open(path: &str) -> i32;
    fn ddriver_close(fd: i32) -> i32;
    fn ddriver_seek(fd: i32, offset: i64, whence: i32) -> i32;
    fn ddriver_write(fd: i32, buf: &[u8], size: usize) -> i32;
    fn ddriver_read(fd: i32, buf: &[u8], size: usize) -> i32;
    fn ddriver_ioctl(fd: i32, cmd: u32, arg: &[u8]) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
}
