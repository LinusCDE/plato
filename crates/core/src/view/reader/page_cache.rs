//! Rendered pages of fixed-layout documents, kept on disk.
//!
//! Scanned books can take seconds per page to render on a reMarkable 2, so
//! every rendered page is stored (zstd compressed) and reused the next time
//! it's needed, including after Plato restarts. Only the book being read is
//! kept: opening a book removes the pages of every other one. The cache is
//! also bounded (100 MB, about 280 scanned pages), and the least recently
//! used pages are removed first, while reading as well as on opening.
//!
//! PLATO_PAGE_CACHE=0 disables it, PLATO_PAGE_CACHE_MB sets its size.

use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use crate::framebuffer::Pixmap;
use super::Resource;
use super::prefetch::RenderParams;

const MAGIC: &[u8; 8] = b"PLATOPC1";
const DEFAULT_LIMIT_MB: u64 = 100;

pub struct PageCache {
    pub dir: PathBuf,
    // Bytes this book's pages take, kept up to date as pages are stored.
    used: AtomicU64,
}

fn limit() -> u64 {
    std::env::var("PLATO_PAGE_CACHE_MB").ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LIMIT_MB) * 1024 * 1024
}

fn cache_root() -> Option<PathBuf> {
    if std::env::var("PLATO_PAGE_CACHE").is_ok_and(|v| v == "0") {
        return None;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(".cache/plato/pages"))
}

fn params_hash(params: &RenderParams) -> u64 {
    let mut h = DefaultHasher::new();
    params.rect.hash(&mut h);
    let m = &params.cropping_margin;
    for v in [m.top, m.right, m.bottom, m.left] {
        v.to_bits().hash(&mut h);
    }
    params.margin_width.hash(&mut h);
    format!("{:?}", params.zoom_mode).hash(&mut h);
    h.finish()
}

impl PageCache {
    pub fn new(doc_path: &Path) -> Option<PageCache> {
        let root = cache_root()?;
        // Same file, same size, same modification time: same pages.
        let meta = fs::metadata(doc_path).ok()?;
        let mut h = DefaultHasher::new();
        doc_path.hash(&mut h);
        meta.len().hash(&mut h);
        meta.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs().hash(&mut h);
        let dir = root.join(format!("{:016x}", h.finish()));
        fs::create_dir_all(&dir).ok()?;
        let used = cached_files(&dir).iter().map(|f| f.1).sum();
        Some(PageCache { dir, used: AtomicU64::new(used) })
    }

    fn path(&self, location: usize, params: &RenderParams) -> PathBuf {
        self.dir.join(format!("{}-{:016x}.zst", location, params_hash(params)))
    }

    pub fn contains(&self, location: usize, params: &RenderParams) -> bool {
        self.path(location, params).exists()
    }

    pub fn load(&self, location: usize, params: &RenderParams) -> Option<Resource> {
        let path = self.path(location, params);
        let mut file = File::open(&path).ok()?;
        let mut header = [0u8; 8 + 4 * 8];
        file.read_exact(&mut header).ok()?;
        if &header[..8] != MAGIC {
            return None;
        }
        let word = |i: usize| u32::from_le_bytes(header[8 + 4 * i..12 + 4 * i].try_into().unwrap());
        let (width, height, samples) = (word(0), word(1), word(2) as usize);
        let frame = rect![word(3) as i32, word(4) as i32, word(5) as i32, word(6) as i32];
        let scale = f32::from_bits(word(7));
        let len = width as usize * height as usize * samples;
        let mut data = Vec::with_capacity(len);
        zstd::stream::read::Decoder::new(file).ok()?.read_to_end(&mut data).ok()?;
        if data.len() != len {
            return None;
        }
        // Mark as recently used for the size limit.
        File::options().write(true).open(&path)
            .and_then(|f| f.set_modified(SystemTime::now())).ok();
        Some(Resource { pixmap: Pixmap { width, height, samples, data }, frame, scale })
    }

    pub fn store(&self, location: usize, params: &RenderParams, resource: &Resource) {
        let path = self.path(location, params);
        let tmp = path.with_extension("tmp");
        let result = (|| -> std::io::Result<()> {
            let Resource { pixmap, frame, scale } = resource;
            let mut file = File::create(&tmp)?;
            file.write_all(MAGIC)?;
            for v in [pixmap.width, pixmap.height, pixmap.samples as u32,
                      frame.min.x as u32, frame.min.y as u32, frame.max.x as u32, frame.max.y as u32,
                      scale.to_bits()] {
                file.write_all(&v.to_le_bytes())?;
            }
            let mut encoder = zstd::stream::write::Encoder::new(file, 1)?;
            encoder.write_all(&pixmap.data)?;
            encoder.finish()?;
            fs::rename(&tmp, &path)
        })();
        if result.is_err() {
            fs::remove_file(&tmp).ok();
            return;
        }
        let len = fs::metadata(&path).map_or(0, |m| m.len());
        if self.used.fetch_add(len, Ordering::Relaxed) + len > limit() {
            self.used.store(trim_dir(&self.dir, limit()), Ordering::Relaxed);
        }
    }
}

fn cached_files(dir: &Path) -> Vec<(SystemTime, u64, PathBuf)> {
    walkdir::WalkDir::new(dir)
        .into_iter().filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), e.into_path()))
        }).collect()
}

// Removes the least recently used pages under `dir` until they fit in
// `limit` bytes, and returns how many bytes are left.
fn trim_dir(dir: &Path, limit: u64) -> u64 {
    let mut files = cached_files(dir);
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= limit {
        return total;
    }
    files.sort_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= limit {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
    total
}

// Removes the pages of every book but the one in `keep`, then the least
// recently used pages until the cache fits its limit.
pub fn trim_cache(keep: &Path) {
    let Some(root) = cache_root() else { return };
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path != keep {
                if path.is_dir() {
                    fs::remove_dir_all(&path).ok();
                } else {
                    fs::remove_file(&path).ok();
                }
            }
        }
    }
    trim_dir(&root, limit());
}
