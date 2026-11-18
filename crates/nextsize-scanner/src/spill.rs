//! On-disk LIFO stack for pending directories that do not fit the in-memory
//! frontier. Record layout: [path bytes][id u64][depth u32][path_len u32].
//! The file is private to the process and deleted on drop.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::engine::DirTask;

pub struct SpillStack {
    file: File,
    path: PathBuf,
    len: u64,
    count: u64,
}

const TAIL: u64 = 16;

impl SpillStack {
    pub fn create(dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = dir.join(format!("nexttool-spill-{}-{nanos}.bin", std::process::id()));
        let file = OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
        Ok(SpillStack { file, path, len: 0, count: 0 })
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn push(&mut self, t: &DirTask) -> io::Result<()> {
        let bytes = t.path.as_os_str().as_encoded_bytes();
        let plen = u32::try_from(bytes.len()).map_err(|_| io::Error::other("path too long for spill"))?;
        let mut rec = Vec::with_capacity(bytes.len() + TAIL as usize);
        rec.extend_from_slice(bytes);
        rec.extend_from_slice(&t.id.to_le_bytes());
        rec.extend_from_slice(&t.depth.to_le_bytes());
        rec.extend_from_slice(&plen.to_le_bytes());
        self.file.seek(SeekFrom::Start(self.len))?;
        self.file.write_all(&rec)?;
        self.len += rec.len() as u64;
        self.count += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> io::Result<Option<DirTask>> {
        if self.count == 0 {
            return Ok(None);
        }
        let mut tail = [0u8; TAIL as usize];
        self.file.seek(SeekFrom::Start(self.len - TAIL))?;
        self.file.read_exact(&mut tail)?;
        let id = u64::from_le_bytes(tail[0..8].try_into().unwrap());
        let depth = u32::from_le_bytes(tail[8..12].try_into().unwrap());
        let plen = u32::from_le_bytes(tail[12..16].try_into().unwrap()) as u64;
        let start = self.len - TAIL - plen;
        let mut buf = vec![0u8; plen as usize];
        self.file.seek(SeekFrom::Start(start))?;
        self.file.read_exact(&mut buf)?;
        self.len = start;
        self.count -= 1;
        if self.count == 0 {
            self.file.set_len(0)?;
            self.len = 0;
        }
        // SAFETY: bytes were produced by `as_encoded_bytes` in this same process.
        let os = unsafe { OsString::from_encoded_bytes_unchecked(buf) };
        Ok(Some(DirTask { id, depth, path: PathBuf::from(os) }))
    }
}

impl Drop for SpillStack {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifo_roundtrip_with_unicode() {
        let t = tempfile::tempdir().unwrap();
        let mut s = SpillStack::create(t.path()).unwrap();
        for i in 0..1000u64 {
            s.push(&DirTask { id: i, depth: i as u32 % 7, path: PathBuf::from(format!("C:\\dir ação ✓\\{i}")) }).unwrap();
        }
        assert_eq!(s.count(), 1000);
        for i in (0..1000u64).rev() {
            let d = s.pop().unwrap().unwrap();
            assert_eq!(d.id, i);
            assert_eq!(d.depth, i as u32 % 7);
            assert_eq!(d.path, PathBuf::from(format!("C:\\dir ação ✓\\{i}")));
        }
        assert!(s.pop().unwrap().is_none());
        let p = s.path.clone();
        drop(s);
        assert!(!p.exists());
    }
}
