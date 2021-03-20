use std::fs;
use std::fs::File;
use std::io::{Error, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Used to sort a temporary file generated by `sort_file` function.
struct FileSortHelper {
    cache_size: u64,
    caches_num: u64,
    buffer_size: u64,
    in_file: File,
    in_file_path: PathBuf,
    out_file: File,
    in_buffers: Vec<Vec<u8>>,
    in_buffers_pos: Vec<u64>,
    in_buffers_index: Vec<u64>,
    out_buffer: Vec<u8>,
    tmp_buffer: Vec<u8>,
    slices_per_cache: u64,
    slices_per_last_cache: u64,
    last_slice_size: u64,
    last_slice_in_last_cache_size: u64,
}

impl FileSortHelper {
    fn new(
        cache_size: u64,
        caches_num: u64,
        in_file: File,
        in_file_len: u64,
        in_file_path: PathBuf,
        out_file: File,
    ) -> Result<Self, Error> {
        let max_caches_num = cache_size - 1;
        assert!(caches_num <= max_caches_num, "File is too big.");
        let buffer_size = cache_size / (caches_num + 1);
        // This should be always true, because we already checked that the file is not empty.
        assert_ne!(buffer_size, 0, "file is not empty; qed");
        let in_buffers = vec![Vec::<u8>::with_capacity(buffer_size as usize); caches_num as usize];
        let in_buffers_pos = vec![buffer_size; caches_num as usize];
        let in_buffers_index = vec![0; caches_num as usize];
        let out_buffer = Vec::<u8>::with_capacity(buffer_size as usize);
        let slices_per_cache = (cache_size + (buffer_size - 1)) / buffer_size;
        let last_cache_size = cache_size - (cache_size * caches_num - in_file_len);
        let slices_per_last_cache = (last_cache_size + (buffer_size - 1)) / buffer_size;
        let last_slice_size = buffer_size - (slices_per_cache * buffer_size - cache_size);
        let last_slice_in_last_cache_size =
            buffer_size - (slices_per_last_cache * buffer_size - last_cache_size);
        let tmp_buffer = vec![0u8; buffer_size as usize];

        let mut sorter = FileSortHelper {
            cache_size,
            caches_num,
            buffer_size,
            in_file,
            in_file_path,
            out_file,
            in_buffers,
            in_buffers_pos,
            in_buffers_index,
            out_buffer,
            tmp_buffer,
            slices_per_cache,
            slices_per_last_cache,
            last_slice_size,
            last_slice_in_last_cache_size,
        };
        sorter.init_buffers()?;
        Ok(sorter)
    }

    /// Merges `in_buffers` into `out_buffer`.
    fn merge(&mut self) -> Result<(), Error> {
        loop {
            let mut min_ind = 0;
            let mut min = u8::MAX;
            let mut changed = false;
            for (i, &pos) in self.in_buffers_pos.iter().enumerate() {
                if let Some(&m) = self.in_buffers[i].get(pos as usize) {
                    if m < min {
                        min = m;
                        min_ind = i;
                        changed = true;
                    }
                }
            }
            // No changes were occurred, which means we merged all the buffers.
            if !changed {
                break;
            }
            self.out_buffer.push(min);
            self.in_buffers_pos[min_ind] += 1;
            if self.in_buffers_pos[min_ind] as usize == self.in_buffers[min_ind].len() {
                self.load_next_buffer(min_ind)?;
            }
            // We filled up the output buffer - write it out and clear.
            if self.out_buffer.len() == self.buffer_size as usize {
                self.out_file.write_all(&self.out_buffer)?;
                self.out_buffer.clear();
            }
        }
        // Write out the rest.
        self.out_file.write_all(&self.out_buffer)?;
        self.out_file.flush()?;
        Ok(())
    }

    /// Loads a corresponding i-th buffer from the input file.
    fn load_next_buffer(&mut self, i: usize) -> Result<(), Error> {
        let in_buff = &mut self.in_buffers[i];
        let is_last_buffer = i == (self.caches_num - 1) as usize;
        let slice_ind = self.in_buffers_index[i];
        let has_next_slice = if !is_last_buffer {
            slice_ind != self.slices_per_cache
        } else {
            slice_ind != self.slices_per_last_cache
        };
        // When we have more slices to read - refill the buffer.
        if has_next_slice {
            in_buff.clear();
            let is_last_slice = if !is_last_buffer {
                slice_ind == (self.slices_per_cache - 1)
            } else {
                slice_ind == (self.slices_per_last_cache - 1)
            };
            let read_len = if !is_last_slice {
                self.buffer_size
            } else if !is_last_buffer {
                self.last_slice_size
            } else {
                self.last_slice_in_last_cache_size
            };
            let read_buff = &mut self.tmp_buffer[..read_len as usize];
            self.in_file.seek(SeekFrom::Start(
                i as u64 * self.cache_size + slice_ind * self.buffer_size,
            ))?;
            self.in_file.read_exact(read_buff)?;
            in_buff.extend_from_slice(read_buff);
            self.in_buffers_pos[i] = 0;
            self.in_buffers_index[i] += 1;
        }
        Ok(())
    }

    /// Initialized buffers.
    fn init_buffers(&mut self) -> Result<(), Error> {
        for (i, in_buff) in self.in_buffers.iter_mut().enumerate() {
            let is_last_buffer = i == (self.caches_num - 1) as usize;
            let slice_ind = self.in_buffers_index[i];
            in_buff.clear();
            let is_last_slice = if !is_last_buffer {
                slice_ind == (self.slices_per_cache - 1)
            } else {
                slice_ind == (self.slices_per_last_cache - 1)
            };
            let read_len = if !is_last_slice {
                self.buffer_size
            } else if !is_last_buffer {
                self.last_slice_size
            } else {
                self.last_slice_in_last_cache_size
            };
            let read_buff = &mut self.tmp_buffer[..read_len as usize];
            self.in_file.read_exact(read_buff)?;
            in_buff.extend_from_slice(read_buff);
            self.in_buffers_pos[i] = 0;
            self.in_buffers_index[i] += 1;
            if !is_last_buffer {
                self.in_file.seek(SeekFrom::Current(
                    (self.cache_size - read_len + slice_ind * self.buffer_size) as i64,
                ))?;
            }
        }
        self.out_buffer.clear();
        Ok(())
    }
}

/// Automatically drop the temporary file.
impl Drop for FileSortHelper {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.in_file_path);
    }
}

/**
Sorts the file content and returns output file path.

File's content is divided by M parts each of size at max of our cache size (`C`) (basically RAM).

_Input file:_
```nocompile
+------------+------------+-----+------------------+
| CACHE SIZE | CACHE SIZE | ... | CACHE SIZE - REM |
+------------+------------+-----+------------------+
```

Each part is loaded to RAM, then sorted and written to a temp file. After this, we create `M` input
buffers and one output buffer. In each buffer we load the first `N=C/M` bytes of each sorted
slice in the temp file.

_Temporary file:_
```nocompile
+--------------+--------------+-----+--------------------+
| SORTED SLICE | SORTED SLICE | ... | SORTED SLICE - REM |
+----+---------+----+---------+-----+----+---------------+   +----+
| IN |         | IN |               | IN |                   | OUT|
+----+         +----+               +----+                   +----+
```
Then all the buffers are merged to the output buffer.

_Buffers:_
```nocompile
+----+   +----+     +----+   +----+
| IN |   | IN | ... | IN |   | OUT|
+----+   +----+     +----+   +----+
 |         |            \-----^^^
 \---------\------------------/ |
            -------------------/
```

Once the output buffer filled, it contents is written to the output file and cleared for the next
merge. Once one of the input buffers is empty, we load the next one and continue the merge process.

_Output file:_
```nocompile
+--------+--------+-----+--------+
| OUT #0 | OUT #1 | ... | OUT #I |
+--------+--------+-----+--------+
```

Maximum file size is: `(cache_size + 1) ** 2` bytes. It can be improved to
`((cache_size + 1) ** 2) * (2 ** 64)` by adding another abstraction over caches, but the idea will be
the same.
*/
pub fn sort_file<P: AsRef<Path>>(path: P, cache_size: u64) -> Result<PathBuf, Error> {
    // Prepare a temporary file.
    let mut cache = Vec::<u8>::with_capacity(cache_size as usize);

    let path = path.as_ref();
    let mut file = fs::File::open(path)?;
    let out_file_path = path.with_extension("tmp.txt");
    let mut file_out = fs::File::create(&out_file_path)?;

    let mut caches_num = 0;
    let mut file_len = 0;
    let mut tmp_buffer = vec![0u8; cache_size as usize];
    loop {
        let n = file.read(&mut tmp_buffer)?;
        if n == 0 {
            break;
        }
        cache.extend_from_slice(&tmp_buffer[..n]);
        cache.sort_unstable();
        file_out.write_all(&cache)?;
        cache.clear();
        file_len += n as u64;
        caches_num += 1;
    }
    if file_len <= 1 {
        println!("File is already sorted.");
        fs::remove_file(out_file_path)?;
        return Ok(path.to_owned());
    }
    // We have sorted the whole file. Return the temporary one.
    if caches_num == 1 {
        drop(file_out);
        drop(file);
        let out_path = path.with_extension("out.txt");
        fs::rename(out_file_path, &out_path)?;
        return Ok(out_path);
    }
    file = fs::File::open(&out_file_path)?;
    // Here we should output to the initial file, but using another one for comparison.
    let file_out_path = path.with_extension("out.txt");
    file_out = fs::File::create(&file_out_path)?;
    // Sort input file using the temporary one.
    let mut sorter = FileSortHelper::new(
        cache_size,
        caches_num,
        file,
        file_len,
        out_file_path,
        file_out,
    )?;
    sorter.merge()?;
    Ok(file_out_path)
}