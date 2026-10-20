//! Direct client for AppLoad's qtfb framebuffer.
//!
//! When Plato is started by AppLoad, the screen is a shared-memory RGB565
//! buffer that xochitl composites onto the e-ink panel. Talking to it
//! directly (instead of through libremarkable and the LD_PRELOAD fb shim)
//! lets us blit whole rows through a lookup table and, crucially, never
//! switch qtfb's refresh mode: AppLoad sleeps for a second after every
//! refresh mode change, and the shim switched modes whenever the waveform
//! of consecutive updates differed.

use super::{Framebuffer, UpdateMode, Pixmap};
use crate::color::Color;
use crate::geom::{Point, Rectangle};
use crate::settings::RefreshQuality;
use anyhow::{Error, format_err};
use std::mem;
use std::os::unix::io::RawFd;
use std::ptr;

const SOCKET_PATH: &[u8] = b"/tmp/qtfb.sock\0";
const MESSAGE_INITIALIZE: u8 = 0;
const MESSAGE_UPDATE: u8 = 1;
const MESSAGE_TERMINATE: u8 = 3;
const MESSAGE_SET_REFRESH_MODE: u8 = 5;
const MESSAGE_REQUEST_FULL_REFRESH: u8 = 6;
const FBFMT_RM2FB: u8 = 0;
const UPDATE_PARTIAL: i32 = 1;

const WIDTH: u32 = 1404;
const HEIGHT: u32 = 1872;

// struct ClientMessage { uint8_t type; union { ... } } is 24 bytes:
// the union starts at offset 4 and its largest member holds five ints.
#[repr(C)]
#[derive(Default)]
struct ClientMessage {
    kind: u8,
    _pad: [u8; 3],
    payload: [i32; 5],
}

// struct ServerMessage { uint8_t type; union { InitMessageResponseContents; UserInputContents; } }
#[repr(C)]
#[derive(Default)]
struct ServerMessage {
    kind: u8,
    _pad: [u8; 3],
    shm_key: i32,
    shm_size: usize,
    _rest: [i32; 3],
}

// Same numbering as the other reMarkable backend (the value of Rot0 is 1).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Rotation {
    Rot270 = 0,
    Rot0 = 1,
    Rot90 = 2,
    Rot180 = 3,
}

impl Rotation {
    fn from_i8(n: i8) -> Result<Rotation, Error> {
        match n {
            0 => Ok(Rotation::Rot270),
            1 => Ok(Rotation::Rot0),
            2 => Ok(Rotation::Rot90),
            3 => Ok(Rotation::Rot180),
            _ => Err(format_err!("invalid rotation {}", n)),
        }
    }
}

pub struct QtfbFramebuffer {
    fd: RawFd,
    shm: *mut u16,
    shm_size: usize,
    rotation: Rotation,
    gray_to_native: [u16; 256],
    full_refresh: bool,
    token: u32,
    monochrome: bool,
    inverted: bool,
    dithered: bool,
    refresh_quality: RefreshQuality,
}

// The mapping is only touched from the thread that owns the framebuffer.
unsafe impl Send for QtfbFramebuffer {}

#[inline]
fn rgb_to_native(r: u8, g: u8, b: u8) -> u16 {
    // Matches libremarkable's rgb_to_native so pixels are bit-identical.
    let r5 = (r as u16 + 1) * 0b11111 / 255;
    let g6 = (g as u16 + 1) * 0b111111 / 255;
    let b5 = (b as u16 + 1) * 0b11111 / 255;
    (r5 << 11) | (g6 << 5) | b5
}

#[inline]
fn native_to_rgb(px: u16) -> [u8; 3] {
    let r5 = px >> 11 & 0b11111;
    let g6 = px >> 5 & 0b111111;
    let b5 = px & 0b11111;
    [(r5 * 255 / 0b11111) as u8, (g6 * 255 / 0b111111) as u8, (b5 * 255 / 0b11111) as u8]
}

#[inline]
fn native_to_gray(px: u16) -> u8 {
    // Green has the most precision and equals the gray level we wrote.
    ((px >> 5 & 0b111111) * 255 / 0b111111) as u8
}

impl QtfbFramebuffer {
    pub fn new() -> Result<QtfbFramebuffer, Error> {
        let key: i32 = std::env::var("QTFB_KEY")
            .map_err(|_| format_err!("QTFB_KEY is not set"))?
            .trim().parse()
            .map_err(|e| format_err!("invalid QTFB_KEY: {}", e))?;

        unsafe {
            let fd = libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0);
            if fd == -1 {
                return Err(std::io::Error::last_os_error().into());
            }
            let fail = |fd: RawFd| -> Error {
                let err = std::io::Error::last_os_error();
                libc::close(fd);
                err.into()
            };

            let mut addr: libc::sockaddr_un = mem::zeroed();
            addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
            for (dst, &src) in addr.sun_path.iter_mut().zip(SOCKET_PATH) {
                *dst = src as libc::c_char;
            }
            if libc::connect(fd, &addr as *const _ as *const libc::sockaddr,
                             mem::size_of::<libc::sockaddr_un>() as libc::socklen_t) != 0 {
                return Err(fail(fd));
            }

            let mut init = ClientMessage { kind: MESSAGE_INITIALIZE, ..Default::default() };
            init.payload[0] = key;
            init.payload[1] = FBFMT_RM2FB as i32;
            if libc::send(fd, &init as *const _ as *const libc::c_void, mem::size_of::<ClientMessage>(), 0) == -1 {
                return Err(fail(fd));
            }

            let mut resp = ServerMessage::default();
            if libc::recv(fd, &mut resp as *mut _ as *mut libc::c_void, mem::size_of::<ServerMessage>(), 0) < 1 {
                return Err(fail(fd));
            }

            let expected = (WIDTH * HEIGHT * 2) as usize;
            if resp.shm_size != expected {
                libc::close(fd);
                return Err(format_err!("unexpected qtfb buffer size {} (expected {})", resp.shm_size, expected));
            }

            let name = format!("/dev/shm/qtfb_{}\0", resp.shm_key);
            let shm_fd = libc::open(name.as_ptr() as *const libc::c_char, libc::O_RDWR | libc::O_CLOEXEC);
            if shm_fd == -1 {
                return Err(fail(fd));
            }
            let shm = libc::mmap(ptr::null_mut(), resp.shm_size, libc::PROT_READ | libc::PROT_WRITE,
                                 libc::MAP_SHARED, shm_fd, 0);
            libc::close(shm_fd);
            if shm == libc::MAP_FAILED {
                return Err(fail(fd));
            }

            let mut gray_to_native = [0u16; 256];
            for (g, px) in gray_to_native.iter_mut().enumerate() {
                *px = rgb_to_native(g as u8, g as u8, g as u8);
            }

            let fb = QtfbFramebuffer {
                fd,
                shm: shm as *mut u16,
                shm_size: resp.shm_size,
                rotation: Rotation::Rot0,
                gray_to_native,
                full_refresh: std::env::var("PLATO_QTFB_FULL_REFRESH").map_or(true, |v| v != "0"),
                token: 0,
                monochrome: false,
                inverted: false,
                dithered: false,
                refresh_quality: Default::default(),
            };

            // Optional: pick one of qtfb's refresh modes once, at startup.
            // Changing it later costs a one second stall in AppLoad.
            if let Some(mode) = std::env::var("PLATO_QTFB_REFRESH_MODE").ok().and_then(|v| v.parse::<i32>().ok()) {
                let mut msg = ClientMessage { kind: MESSAGE_SET_REFRESH_MODE, ..Default::default() };
                msg.payload[0] = mode;
                fb.send(&msg);
            }

            Ok(fb)
        }
    }

    fn send(&self, msg: &ClientMessage) {
        unsafe {
            if libc::send(self.fd, msg as *const _ as *const libc::c_void, mem::size_of::<ClientMessage>(), 0) == -1 {
                eprintln!("qtfb: can't send message {}: {}", msg.kind, std::io::Error::last_os_error());
            }
        }
    }

    #[inline]
    fn buffer(&mut self) -> &mut [u16] {
        unsafe { std::slice::from_raw_parts_mut(self.shm, (WIDTH * HEIGHT) as usize) }
    }

    #[inline]
    fn buffer_ref(&self) -> &[u16] {
        unsafe { std::slice::from_raw_parts(self.shm, (WIDTH * HEIGHT) as usize) }
    }

    // Maps a logical pixel to its index in the physical (portrait) buffer.
    #[inline]
    fn index(&self, x: u32, y: u32) -> usize {
        let (px, py) = match self.rotation {
            Rotation::Rot0 => (x, y),
            Rotation::Rot90 => (WIDTH - 1 - y, x),
            Rotation::Rot180 => (WIDTH - 1 - x, HEIGHT - 1 - y),
            Rotation::Rot270 => (y, HEIGHT - 1 - x),
        };
        (py * WIDTH + px) as usize
    }

    #[inline]
    fn in_bounds(&self, x: u32, y: u32) -> bool {
        x < self.width() && y < self.height()
    }

    #[inline]
    fn get_gray(&self, x: u32, y: u32) -> u8 {
        native_to_gray(self.buffer_ref()[self.index(x, y)])
    }

    #[inline]
    fn put_native(&mut self, x: u32, y: u32, px: u16) {
        let i = self.index(x, y);
        self.buffer()[i] = px;
    }

    // Logical rectangle clipped to the screen, or None if empty.
    fn clip(&self, rect: &Rectangle) -> Option<(u32, u32, u32, u32)> {
        let x0 = rect.min.x.max(0) as u32;
        let y0 = rect.min.y.max(0) as u32;
        let x1 = (rect.max.x.max(0) as u32).min(self.width());
        let y1 = (rect.max.y.max(0) as u32).min(self.height());
        if x0 < x1 && y0 < y1 { Some((x0, y0, x1, y1)) } else { None }
    }

    // Applies `f` to every (destination row slice, logical y, logical x0) of
    // the clipped rectangle when rows are contiguous in memory. Returns false
    // when the current rotation makes rows strided.
    fn for_each_row<F: FnMut(&mut [u16], u32, u32)>(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, mut f: F) -> bool {
        let w = (x1 - x0) as usize;
        match self.rotation {
            Rotation::Rot0 => {
                let buf = self.buffer();
                for y in y0..y1 {
                    let start = (y * WIDTH + x0) as usize;
                    f(&mut buf[start..start + w], y, x0);
                }
                true
            },
            _ => false,
        }
    }

    fn fill(&mut self, rect: &Rectangle, px: u16) {
        let Some((x0, y0, x1, y1)) = self.clip(rect) else { return };
        if !self.for_each_row(x0, y0, x1, y1, |row, _, _| row.fill(px)) {
            for y in y0..y1 {
                for x in x0..x1 {
                    self.put_native(x, y, px);
                }
            }
        }
    }

    // Blits the part of `pixmap` inside `rect` (pixmap coordinates) to `pt`,
    // mapping every gray level through `map`.
    fn blit(&mut self, pixmap: &Pixmap, rect: &Rectangle, pt: Point, map: &[u8; 256]) {
        let dst = rect![pt, pt + (rect.max - rect.min)];
        let Some((x0, y0, x1, y1)) = self.clip(&dst) else { return };
        let sx0 = (rect.min.x + (x0 as i32 - pt.x)) as u32;
        let sy0 = (rect.min.y + (y0 as i32 - pt.y)) as u32;
        let w = (x1 - x0).min(pixmap.width.saturating_sub(sx0));
        let h = (y1 - y0).min(pixmap.height.saturating_sub(sy0));
        if w == 0 || h == 0 {
            return;
        }
        let (x1, y1) = (x0 + w, y0 + h);
        let lut: [u16; 256] = std::array::from_fn(|g| self.gray_to_native[map[g] as usize]);

        if pixmap.samples == 1 {
            let data = &pixmap.data;
            let stride = pixmap.width as usize;
            let rows_done = self.for_each_row(x0, y0, x1, y1, |row, y, _| {
                let src = (sy0 + (y - y0)) as usize * stride + sx0 as usize;
                let len = row.len();
                for (d, &s) in row.iter_mut().zip(&data[src..src + len]) {
                    *d = lut[s as usize];
                }
            });
            if !rows_done {
                for y in y0..y1 {
                    let src = (sy0 + (y - y0)) as usize * stride + sx0 as usize;
                    for x in x0..x1 {
                        let s = data[src + (x - x0) as usize];
                        self.put_native(x, y, lut[s as usize]);
                    }
                }
            }
        } else {
            for y in y0..y1 {
                for x in x0..x1 {
                    let color = pixmap.get_pixel(sx0 + (x - x0), sy0 + (y - y0));
                    let px = match color {
                        Color::Gray(g) => lut[g as usize],
                        Color::Rgb(r, g, b) => rgb_to_native(map[r as usize], map[g as usize], map[b as usize]),
                    };
                    self.put_native(x, y, px);
                }
            }
        }
    }
}

const IDENTITY: [u8; 256] = {
    let mut m = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        m[i] = i as u8;
        i += 1;
    }
    m
};

impl Framebuffer for QtfbFramebuffer {
    fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if !self.in_bounds(x, y) {
            return;
        }
        let px = match color {
            Color::Gray(g) => self.gray_to_native[g as usize],
            Color::Rgb(r, g, b) => rgb_to_native(r, g, b),
        };
        self.put_native(x, y, px);
    }

    fn set_blended_pixel(&mut self, x: u32, y: u32, color: Color, alpha: f32) {
        if !self.in_bounds(x, y) {
            return;
        }
        if alpha >= 1.0 {
            self.set_pixel(x, y, color);
            return;
        }
        let i = self.index(x, y);
        let [r0, g0, b0] = native_to_rgb(self.buffer_ref()[i]);
        let [r1, g1, b1] = match color {
            Color::Rgb(r, g, b) => [r, g, b],
            Color::Gray(g) => [g, g, g],
        };
        let mix = |a: u8, b: u8| (b as f32 * alpha + (1.0 - alpha) * a as f32) as u8;
        self.buffer()[i] = rgb_to_native(mix(r0, r1), mix(g0, g1), mix(b0, b1));
    }

    fn invert_region(&mut self, rect: &Rectangle) {
        let Some((x0, y0, x1, y1)) = self.clip(rect) else { return };
        for y in y0..y1 {
            for x in x0..x1 {
                let g = self.get_gray(x, y);
                let px = self.gray_to_native[(255 - g) as usize];
                self.put_native(x, y, px);
            }
        }
    }

    fn shift_region(&mut self, rect: &Rectangle, drift: u8) {
        let Some((x0, y0, x1, y1)) = self.clip(rect) else { return };
        for y in y0..y1 {
            for x in x0..x1 {
                let g = self.get_gray(x, y).saturating_sub(drift);
                let px = self.gray_to_native[g as usize];
                self.put_native(x, y, px);
            }
        }
    }

    fn draw_rectangle(&mut self, rect: &Rectangle, color: Color) {
        let px = match color {
            Color::Gray(g) => self.gray_to_native[g as usize],
            Color::Rgb(r, g, b) => rgb_to_native(r, g, b),
        };
        self.fill(rect, px);
    }

    fn draw_pixmap(&mut self, pixmap: &Pixmap, pt: Point) {
        self.blit(pixmap, &pixmap.rect(), pt, &IDENTITY);
    }

    fn draw_framed_pixmap(&mut self, pixmap: &Pixmap, rect: &Rectangle, pt: Point) {
        self.blit(pixmap, rect, pt, &IDENTITY);
    }

    fn draw_framed_pixmap_contrast(&mut self, pixmap: &Pixmap, rect: &Rectangle, pt: Point, exponent: f32, gray: f32) {
        if (exponent - 1.0).abs() < f32::EPSILON {
            self.blit(pixmap, rect, pt, &IDENTITY);
            return;
        }
        // Same curve as the generic implementation, evaluated once per level.
        let rem_gray = 255.0 - gray;
        let inv_exponent = 1.0 / exponent;
        let map: [u8; 256] = std::array::from_fn(|i| {
            let c = i as f32;
            if c < gray {
                (gray * (c / gray).powf(exponent)) as u8
            } else if c > gray {
                (gray + rem_gray * ((c - gray) / rem_gray).powf(inv_exponent)) as u8
            } else {
                gray as u8
            }
        });
        self.blit(pixmap, rect, pt, &map);
    }

    fn update(&mut self, rect: &Rectangle, mode: UpdateMode) -> Result<u32, Error> {
        let Some((x0, y0, x1, y1)) = self.clip(rect) else { return Ok(self.token) };
        // Map the two corner pixels to the physical buffer.
        let (w, h) = (WIDTH as i32, HEIGHT as i32);
        let corner = |x: i32, y: i32| -> (i32, i32) {
            match self.rotation {
                Rotation::Rot0 => (x, y),
                Rotation::Rot90 => (w - 1 - y, x),
                Rotation::Rot180 => (w - 1 - x, h - 1 - y),
                Rotation::Rot270 => (y, h - 1 - x),
            }
        };
        let (ax, ay) = corner(x0 as i32, y0 as i32);
        let (bx, by) = corner(x1 as i32 - 1, y1 as i32 - 1);
        let mut msg = ClientMessage { kind: MESSAGE_UPDATE, ..Default::default() };
        msg.payload = [UPDATE_PARTIAL, ax.min(bx), ay.min(by),
                       (ax - bx).abs() + 1, (ay - by).abs() + 1];
        self.send(&msg);

        if mode == UpdateMode::Full && self.full_refresh {
            self.send(&ClientMessage { kind: MESSAGE_REQUEST_FULL_REFRESH, ..Default::default() });
        }

        self.token = self.token.wrapping_add(1);
        Ok(self.token)
    }

    fn wait(&self, _token: u32) -> Result<i32, Error> {
        // The buffer is plain shared memory: drawing never has to wait for
        // the panel. xochitl schedules the e-ink refresh itself.
        Ok(0)
    }

    fn save(&self, path: &str) -> Result<(), Error> {
        let (width, height) = self.dims();
        let mut pixmap = Pixmap::new(width, height, 1);
        for y in 0..height {
            for x in 0..width {
                pixmap.data[(y * width + x) as usize] = self.get_gray(x, y);
            }
        }
        pixmap.save(path)
    }

    fn rotation(&self) -> i8 {
        self.rotation as i8
    }

    fn set_rotation(&mut self, n: i8) -> Result<(u32, u32), Error> {
        self.rotation = Rotation::from_i8(n)?;
        Ok(self.dims())
    }

    fn set_monochrome(&mut self, enable: bool) {
        self.monochrome = enable;
    }

    fn set_dithered(&mut self, enable: bool) {
        self.dithered = enable;
    }

    fn set_inverted(&mut self, enable: bool) {
        self.inverted = enable;
    }

    fn set_refresh_quality(&mut self, quality: RefreshQuality) {
        self.refresh_quality = quality;
    }

    fn monochrome(&self) -> bool {
        self.monochrome
    }

    fn dithered(&self) -> bool {
        self.dithered
    }

    fn inverted(&self) -> bool {
        self.inverted
    }

    fn refresh_quality(&self) -> RefreshQuality {
        self.refresh_quality.clone()
    }

    fn width(&self) -> u32 {
        match self.rotation {
            Rotation::Rot90 | Rotation::Rot270 => HEIGHT,
            _ => WIDTH,
        }
    }

    fn height(&self) -> u32 {
        match self.rotation {
            Rotation::Rot90 | Rotation::Rot270 => WIDTH,
            _ => HEIGHT,
        }
    }
}

impl Drop for QtfbFramebuffer {
    fn drop(&mut self) {
        self.send(&ClientMessage { kind: MESSAGE_TERMINATE, ..Default::default() });
        unsafe {
            libc::munmap(self.shm as *mut libc::c_void, self.shm_size);
            libc::close(self.fd);
        }
    }
}
