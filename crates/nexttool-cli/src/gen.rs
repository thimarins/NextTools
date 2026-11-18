//! Deterministic synthetic tree generator for benchmarks and tests.
//! Files are tiny (≤ max_size bytes, default 512 → resident in the NTFS MFT,
//! so 1 M files do not consume ~4 GB of clusters). Synthetic data is NOT a
//! substitute for real filesystems; results are labeled as synthetic.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

pub struct GenSpec {
    pub files: u64,
    pub files_per_dir: u64,
    pub fanout: u64,
    pub max_size: u64,
    pub seed: u64,
}

pub struct GenResult {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

const EXTS: [&str; 10] = ["txt", "log", "pdf", "docx", "jpg", "bak", "dat", "csv", "mp4", "zip"];

pub fn generate(root: &Path, spec: &GenSpec, mut progress: impl FnMut(u64)) -> io::Result<GenResult> {
    fs::create_dir_all(root)?;
    let mut rng = Rng(spec.seed.max(1) * 0x9E37_79B9_7F4A_7C15);
    let mut res = GenResult { files: 0, dirs: 0, bytes: 0 };
    // BFS over directory indices; directory i's path is derived from i (no big queue kept).
    let mut queue: std::collections::VecDeque<PathBuf> = std::collections::VecDeque::from([root.to_path_buf()]);
    while res.files < spec.files {
        let Some(dir) = queue.pop_front() else { break };
        for f in 0..spec.files_per_dir {
            if res.files >= spec.files {
                break;
            }
            let r = rng.next();
            let ext = EXTS[(r % EXTS.len() as u64) as usize];
            let name = if r % 97 == 0 { format!("relatório ação ✓ {f}.{ext}") } else { format!("file_{f:05}.{ext}") };
            let size = if spec.max_size == 0 { 0 } else { (r >> 8) % (spec.max_size + 1) };
            let file = File::create(dir.join(name))?;
            if size > 0 {
                file.set_len(size)?;
            }
            res.files += 1;
            res.bytes += size;
            if res.files % 10_000 == 0 {
                progress(res.files);
            }
        }
        for d in 0..spec.fanout {
            if res.files >= spec.files && queue.len() as u64 > spec.fanout {
                break;
            }
            let p = dir.join(format!("dir_{d:03}"));
            fs::create_dir(&p)?;
            res.dirs += 1;
            queue.push_back(p);
        }
    }
    Ok(res)
}
