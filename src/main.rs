use std::fs;
use std::io::Error;

pub use big_file_sort::sort_file;

/// Size of available memory. Used by caches.
pub const MEM_SIZE_BYTES: u64 = 64;

fn main() -> Result<(), Error> {
    let _ = sort_file("big_file.txt", MEM_SIZE_BYTES)?;
    Ok(())
}

#[test]
fn should_sort() -> Result<(), Error> {
    let file_name = "big_file.txt";
    let mut v0 = fs::read(file_name)?;
    v0.sort();
    let sorted_path = sort_file(file_name, MEM_SIZE_BYTES)?;
    let v1 = fs::read(sorted_path)?;
    assert_eq!(v0, v1);
    Ok(())
}
