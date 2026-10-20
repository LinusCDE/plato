//! Background page rendering for fixed-layout documents.
//!
//! A worker thread with its own instance of the document renders the pages
//! the reader is likely to show next, so that a page turn only has to blit
//! an already rendered pixmap. If the requested page is being rendered when
//! the reader needs it, the reader waits for that render instead of doing
//! the same work a second time on the UI thread.
//!
//! Every page the worker renders also goes to the on-disk page cache, and
//! once the nearby pages are ready it keeps rendering further ahead into
//! that cache, so even quick page turns usually find their page done.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use crate::document::{open, Document, Location};
use crate::framebuffer::{Framebuffer, Pixmap};
use crate::geom::Rectangle;
use crate::metadata::{Margin, ZoomMode};
use crate::device::CURRENT_DEVICE;
use super::{scaling_factor, Resource};
use super::page_cache::{PageCache, trim_cache};

// Rendered pages waiting to be picked up (each one is ~2.6 MB on an rM2).
const MAX_READY: usize = 4;

// Everything a page's pixmap depends on. Results rendered with different
// parameters (after a zoom, crop or rotation change) are discarded.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderParams {
    pub rect: Rectangle,
    pub cropping_margin: Margin,
    pub margin_width: i32,
    pub zoom_mode: ZoomMode,
}

pub fn render_resource(doc: &mut dyn Document, location: usize, params: &RenderParams) -> Resource {
    let RenderParams { rect, cropping_margin, margin_width, zoom_mode } = params;
    let dims = doc.dims(location).unwrap_or((3.0, 4.0));
    let scale = scaling_factor(rect, cropping_margin, *margin_width, dims, *zoom_mode);
    if let Some((pixmap, _)) = doc.pixmap(Location::Exact(location), scale, CURRENT_DEVICE.color_samples()) {
        let frame = rect![(cropping_margin.left * pixmap.width as f32).ceil() as i32,
                          (cropping_margin.top * pixmap.height as f32).ceil() as i32,
                          ((1.0 - cropping_margin.right) * pixmap.width as f32).floor() as i32,
                          ((1.0 - cropping_margin.bottom) * pixmap.height as f32).floor() as i32];
        Resource { pixmap, frame, scale }
    } else {
        let width = (dims.0 * scale).max(1.0) as u32;
        let height = (dims.1 * scale).max(1.0) as u32;
        let pixmap = Pixmap::empty(width, height, CURRENT_DEVICE.color_samples());
        let frame = pixmap.rect();
        Resource { pixmap, frame, scale }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Target {
    Memory, // Wanted soon: keep the result for the reader.
    Disk,   // Further ahead: only make sure it's in the page cache.
}

struct Job {
    location: usize,
    params: RenderParams,
    target: Target,
}

#[derive(Default)]
struct State {
    queue: Vec<Job>,  // Highest priority first.
    in_flight: Option<(usize, RenderParams)>,
    ready: Vec<(usize, RenderParams, Resource, Target)>,
    quit: bool,
}

pub struct Prefetcher {
    shared: Arc<(Mutex<State>, Condvar)>,
    page_cache: Option<Arc<PageCache>>,
}

impl Prefetcher {
    pub fn new(path: PathBuf) -> Prefetcher {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let shared2 = Arc::clone(&shared);
        let page_cache = PageCache::new(&path).map(Arc::new);
        let page_cache2 = page_cache.clone();

        thread::Builder::new().name("prefetch".to_string()).spawn(move || {
            // Stay out of the way of the UI thread on this dual core device.
            unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 5); }

            let (lock, cvar) = &*shared2;
            let Some(mut doc) = open(&path) else {
                eprintln!("Prefetch: can't open {}.", path.display());
                lock.lock().unwrap().quit = true;
                cvar.notify_all();
                return;
            };

            loop {
                let job = {
                    let mut state = lock.lock().unwrap();
                    while state.queue.is_empty() && !state.quit {
                        state = cvar.wait(state).unwrap();
                    }
                    if state.quit {
                        break;
                    }
                    let job = state.queue.remove(0);
                    state.in_flight = Some((job.location, job.params.clone()));
                    job
                };

                let cached = page_cache2.as_ref().and_then(|pc| {
                    if job.target == Target::Disk {
                        // Already there: nothing to do.
                        pc.contains(job.location, &job.params).then_some(None)
                    } else {
                        timed!(format!("disk p{} (prefetch)", job.location),
                               pc.load(job.location, &job.params)).map(Some)
                    }
                });

                let resource = match cached {
                    Some(Some(resource)) => Some(resource),
                    Some(None) => None,
                    None => {
                        let resource = timed!(format!("pixmap p{} (prefetch)", job.location),
                                              render_resource(doc.as_mut(), job.location, &job.params));
                        if let Some(pc) = page_cache2.as_ref() {
                            timed!(format!("store p{}", job.location), pc.store(job.location, &job.params, &resource));
                        }
                        Some(resource)
                    },
                };

                let mut state = lock.lock().unwrap();
                state.in_flight = None;
                if let Some(resource) = resource {
                    state.ready.push((job.location, job.params, resource, job.target));
                    while state.ready.len() > MAX_READY {
                        // Drop look-ahead results before the ones asked for.
                        let i = state.ready.iter().position(|r| r.3 == Target::Disk).unwrap_or(0);
                        state.ready.remove(i);
                    }
                }
                cvar.notify_all();
            }
        }).ok();

        // Drop the other books' pages and keep the cache within its size
        // limit, at the lowest priority.
        if let Some(keep) = page_cache.as_ref().map(|pc| pc.dir.clone()) {
            thread::spawn(move || {
                unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 19); }
                trim_cache(&keep);
            });
        }

        Prefetcher { shared, page_cache }
    }

    // Replaces the pending work: `soon` are kept in memory (highest priority
    // first), `ahead` are only rendered into the page cache.
    pub fn request(&self, soon: &[usize], ahead: &[usize], params_for: impl Fn(usize) -> RenderParams) {
        let (lock, cvar) = &*self.shared;
        let mut state = lock.lock().unwrap();
        if state.quit {
            return;
        }
        let wanted = soon.iter().map(|&l| (l, Target::Memory))
                         .chain(ahead.iter().map(|&l| (l, Target::Disk)));
        let jobs: Vec<Job> = wanted.filter_map(|(location, target)| {
            let params = params_for(location);
            let done = state.ready.iter().any(|(l, p, _, _)| *l == location && *p == params) ||
                       state.in_flight.as_ref().is_some_and(|(l, p)| *l == location && *p == params);
            if done { None } else { Some(Job { location, params, target }) }
        }).collect();
        state.queue = jobs;
        // Results nobody asked for any more only hold on to memory.
        state.ready.retain(|(l, _, _, _)| soon.contains(l) || ahead.contains(l));
        cvar.notify_all();
    }

    // Returns the prefetched page, waiting if it is being rendered right now.
    pub fn take(&self, location: usize, params: &RenderParams) -> Option<Resource> {
        let (lock, cvar) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.queue.retain(|job| job.location != location);
        loop {
            if let Some(i) = state.ready.iter().position(|(l, p, _, _)| *l == location && p == params) {
                return Some(state.ready.remove(i).2);
            }
            let rendering = state.in_flight.as_ref().is_some_and(|(l, p)| *l == location && p == params);
            if !rendering || state.quit {
                return None;
            }
            state = cvar.wait(state).unwrap();
        }
    }

    pub fn page_cache(&self) -> Option<&Arc<PageCache>> {
        self.page_cache.as_ref()
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.quit = true;
        state.queue.clear();
        cvar.notify_all();
    }
}
