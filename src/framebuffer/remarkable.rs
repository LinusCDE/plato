use std::ops::Drop;
use anyhow::Error;
use crate::geom::Rectangle;
use crate::device::CURRENT_DEVICE;
use super::{UpdateMode, Framebuffer};
use libremarkable;
use libremarkable::framebuffer::refresh::PartialRefreshMode;
use libremarkable::framebuffer::FramebufferBase;
use libremarkable::framebuffer::FramebufferIO;
use libremarkable::framebuffer::FramebufferRefresh;
use libremarkable::framebuffer::cgmath;
use libremarkable::framebuffer::common;
use std::convert::TryInto;
use crate::settings::RefreshQuality;
use std::fs;
use memmap2::MmapOptions;

type ColorTransform = fn(u32, u32, u8) -> u8; // Copied from ./kobo.rs

pub struct RemarkableFramebuffer {
    fb: libremarkable::framebuffer::core::Framebuffer<'static>,
    monochrome: bool, // Currently stubbed
    inverted: bool, // Currently stubbed
    transform: ColorTransform,
    dithered: bool,
    refresh_quality: RefreshQuality,
}

impl RemarkableFramebuffer {
    pub fn new(fb_device_path: &'static str) -> Result<RemarkableFramebuffer, Error> {
        Ok(RemarkableFramebuffer {
            fb: libremarkable::framebuffer::core::Framebuffer::from_path(fb_device_path),
            monochrome: false,
            inverted: false,
            dithered: false,
            transform: transform_identity,
            refresh_quality: Default::default()
        })
    }

    fn set_pixel_rgb(&mut self, x: u32, y: u32, rgb: [u8; 3]) {
        let [red, green, blue] = rgb;
        /*if self.fb.var_screen_info.rotate % 2 == 0 {
            // Swap x and y
            self.fb.write_pixel(cgmath::Point2 { x: y as i32, y: x as i32 }, common::color::RGB(red, green, blue));
        }else {*/
            self.fb.write_pixel(cgmath::Point2 { x: x as i32, y: y as i32 }, common::color::RGB(red, green, blue));
        //}
    }

    fn get_pixel_rgb(&self, x: u32, y: u32) -> [u8; 3] {
        self.fb.read_pixel(cgmath::Point2 { x, y }).to_rgb8()
    }
}

impl Framebuffer for RemarkableFramebuffer {
    fn refresh_quality(&self) -> RefreshQuality {
        self.refresh_quality.clone()
    }

    fn set_refresh_quality(&mut self, quality: RefreshQuality) {
        self.refresh_quality = quality;
    }

    fn set_pixel(&mut self, x: u32, y: u32, color: u8) {
        // color seems to be inverted! Either in color::GRAY or from plato.
        /*if self.fb.var_screen_info.rotate % 2 == 0 {
            // Swap x and y
            self.fb.write_pixel(cgmath::Point2 { x: y as i32, y: x as i32 }, common::color::GRAY(255 - color));
        }else {*/
            self.fb.write_pixel(cgmath::Point2 { x: x as i32, y: y as i32 }, common::color::GRAY(255 - color));
        //}
    }

    fn set_blended_pixel(&mut self, x: u32, y: u32, color: u8, alpha: f32) {
        if alpha >= 1.0 {
            self.set_pixel(x, y, color);
            return;
        }
        let rgb = self.get_pixel_rgb(x, y);
        let color_alpha = color as f32 * alpha;
        let red = color_alpha + (1.0 - alpha) * rgb[0] as f32;
        let green = color_alpha + (1.0 - alpha) * rgb[1] as f32;
        let blue = color_alpha + (1.0 - alpha) * rgb[2] as f32;
        self.set_pixel_rgb(x, y, [red as u8, green as u8, blue as u8]);
    }

    fn invert_region(&mut self, rect: &Rectangle) {
        for y in rect.min.y..rect.max.y {
            for x in rect.min.x..rect.max.x {
                let rgb = self.get_pixel_rgb(x as u32, y as u32);
                let red = 255 - rgb[0];
                let green = 255 - rgb[1];
                let blue = 255 - rgb[2];
                self.set_pixel_rgb(x as u32, y as u32, [red, green, blue]);
            }
        }
    }

    fn shift_region(&mut self, rect: &Rectangle, drift: u8) {
        for y in rect.min.y..rect.max.y {
            for x in rect.min.x..rect.max.x {
                let rgb = self.get_pixel_rgb(x as u32, y as u32);
                let red = rgb[0].saturating_sub(drift);
                let green = rgb[1].saturating_sub(drift);
                let blue = rgb[2].saturating_sub(drift);
                self.set_pixel_rgb(x as u32, y as u32, [red, green, blue]);
            }
        }
    }

    /// Tell the driver that the screen needs to be redrawn.
    /// The returned u32 (if Ok) is called a token that seems to
    /// represent this particular refresh job. Whether this update 
    /// is done can be checked using that token.
    fn update(&mut self, rect: &Rectangle, mode: UpdateMode) -> Result<u32, Error> {
        let new_rect = common::mxcfb_rect {
            left: rect.min.x as u32,
            top: rect.min.y as u32,
            width: rect.max.x as u32 - rect.min.x as u32,
            height: rect.max.y as u32 - rect.min.y as u32,
        };

        let overwrite_dither = if self.dithered {
            Some(common::dither_mode::EPDC_FLAG_USE_DITHERING_Y1)
        } else {
            None
        };


        // Note: I took some of the comments from libremarkable
        // regarding those settings and rephrased them here for
        // easier lookup. Please also look up the original comments
        // as they will be far more helpful and practical.
        match mode {
            UpdateMode::FastMono => {
                //println!("Update fastmono");
                Ok(self.fb.partial_refresh(
                    &new_rect,
                    PartialRefreshMode::Async,
                    common::waveform_mode::WAVEFORM_MODE_DU,
                    common::display_temp::TEMP_USE_REMARKABLE_DRAW, // Low latency (see comments on this)
                    overwrite_dither.unwrap_or(common::dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH),
                    common::DRAWING_QUANT_BIT,
                    false,
                ))
            },
            UpdateMode::Fast => {
                //println!("Update fast");
                Ok(self.fb.partial_refresh(
                    &new_rect,
                    PartialRefreshMode::Async,
                    common::waveform_mode::WAVEFORM_MODE_GLR16,
                    common::display_temp::TEMP_USE_AMBIENT,
                    overwrite_dither.unwrap_or(common::dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH),
                    common::DRAWING_QUANT_BIT,
                    false,
                ))
            },
            UpdateMode::Gui => {
                //println!("Update gui");
                Ok(self.fb.partial_refresh(
                    &new_rect,
                    PartialRefreshMode::Async,
                    common::waveform_mode::WAVEFORM_MODE_GC16_FAST, // Also used by reMarkable for UI (anymore??)
                    common::display_temp::TEMP_USE_AMBIENT,
                    overwrite_dither.unwrap_or(common::dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH),
                    common::DRAWING_QUANT_BIT,
                    false,
                ))
            },
            UpdateMode::Partial => {
                //println!("Update partial");
                // EPDC_FLAG_USE_REMARKABLE_DITHER most likely leads to problems here!
                match self.refresh_quality {
                    RefreshQuality::Fast => {
                        // Try to be quick
                        Ok(self.fb.partial_refresh(
                            &new_rect,
                            PartialRefreshMode::Async,
                            common::waveform_mode::WAVEFORM_MODE_GC16_FAST, // Ui setting
                            common::display_temp::TEMP_USE_REMARKABLE_DRAW, // Low latency
                            overwrite_dither.unwrap_or(common::dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH),
                            common::DRAWING_QUANT_BIT,
                            false,
                        ))
                    }
                    RefreshQuality::Normal => {
                        // Not the fastest but decent for epubs
                        Ok(self.fb.partial_refresh(
                            &new_rect,
                            PartialRefreshMode::Async,
                            common::waveform_mode::WAVEFORM_MODE_AUTO,
                            common::display_temp::TEMP_USE_AMBIENT,
                            overwrite_dither.unwrap_or(common::dither_mode::EPDC_FLAG_USE_REMARKABLE_DITHER),
                            common::DRAWING_QUANT_BIT,
                            false,
                        ))
                    },
                    RefreshQuality::Better => {
                        // "Fast" full refreshes. Eliminates more ghosting in mangas with dark scenes
                        Ok(self.fb.partial_refresh(
                            &new_rect,
                            PartialRefreshMode::Async,
                            common::waveform_mode::WAVEFORM_MODE_GC16,
                            common::display_temp::TEMP_USE_AMBIENT,
                            overwrite_dither.unwrap_or(common::dither_mode::EPDC_FLAG_USE_REMARKABLE_DITHER),
                            common::DRAWING_QUANT_BIT,
                            true, // <-- Force full refresh
                        ))
                    },
                    RefreshQuality::Perfect => {
                        // Even more agressive full refreshes. Should eliminate all ghosting
                        Ok(self.fb.partial_refresh(
                            &new_rect,
                            PartialRefreshMode::Async,
                            common::waveform_mode::WAVEFORM_MODE_GC16,
                            common::display_temp::TEMP_USE_MAX,
                            overwrite_dither.unwrap_or(common::dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH),
                            common::DRAWING_QUANT_BIT,
                            true, // <-- Force full refresh
                        ))
                    }
                }
            },
            UpdateMode::Full => {
                //println!("Update full");
                Ok(self.fb.full_refresh(
                    common::waveform_mode::WAVEFORM_MODE_GC16, // Flashes black white in full mode
                    common::display_temp::TEMP_USE_AMBIENT, // Not such low latency (see comments on this)
                    overwrite_dither.unwrap_or(common::dither_mode::EPDC_FLAG_USE_REMARKABLE_DITHER), // Good or bad here???
                    common::DRAWING_QUANT_BIT,
                    false, // Don't wait for completion (token should allow the device to do anyway if actually wanted)
                ))
            }
        }
    }

    // Wait for a specific update to complete.
    fn wait(&self, token: u32) -> Result<i32, Error> {
        Ok(self.fb.wait_refresh_complete(token).try_into().expect("Converting libremarkables u32 to i32 went out of scope when waiting for a refresh to complete"))
    }

    fn save(&self, path: &str) -> Result<(), Error> {
        let rgb565 = self.fb.dump_region(common::mxcfb_rect {
                top: 0,
                left: 0,
                width: self.fb.var_screen_info.xres,
                height: self.fb.var_screen_info.yres,
            }).unwrap();

        let rgb888 = libremarkable::framebuffer::storage::rgbimage_from_u8_slice(
            self.fb.var_screen_info.xres,
            self.fb.var_screen_info.yres,
            &rgb565,
        )
        .unwrap();

        let mut writer = std::io::BufWriter::new(Vec::new());
        libremarkable::image::png::PngEncoder::new(&mut writer)
            .encode(
                &*rgb888,
                self.fb.var_screen_info.xres,
                self.fb.var_screen_info.yres,
                libremarkable::image::ColorType::Rgb8,
            )
            .unwrap();

        let png = writer.into_inner().unwrap();
        fs::write(path, &*png)?;
        Ok(())        
    }

    #[inline]
    fn rotation(&self) -> i8 {
        self.fb.var_screen_info.rotate as i8
    }

    fn set_rotation(&mut self, n: i8) -> Result<(u32, u32), Error> {
        // This will probably not work.
        // Not sure if the result is even correct.

        self.fb.var_screen_info.rotate = n as u32;
        self.fb.update_var_screeninfo();

        // If this is not done, the frame will be garbled
        // Kindly taken from libremarkable::framebuffer::core::Framebuffer::new()
        self.fb.fix_screen_info = libremarkable::framebuffer::core::Framebuffer::get_fix_screeninfo(&self.fb.device, self.fb.swtfb_client.as_ref()); // Seems to change
        let frame_length = (self.fb.fix_screen_info.line_length * self.fb.var_screen_info.yres) as usize;
        let mem_map = MmapOptions::new()
                .len(frame_length)
                .map_raw(&self.fb.device)
                .expect("Unable to map provided path");
        self.fb.frame = mem_map;

        Ok((self.width(), self.height())) // With and height have already updated
    }

    fn set_inverted(&mut self, enable: bool) {
        self.inverted = enable;
    }

    fn inverted(&self) -> bool {
        self.inverted
    }

    fn set_monochrome(&mut self, enable: bool) {
        // As I understand it, monochrome mode
        // is back and white only (no "gray").
        // should yield faster refreshes.
        // libremarkable has similar EPDC flags
        // but I'm not yet sure where to put/or them.
        
        // Currently stubbed
        self.monochrome = enable
    }

    fn monochrome(&self) -> bool {
        self.monochrome
    }

    fn set_dithered(&mut self, enable: bool) {
        if enable == self.dithered {
            return;
        }

        self.dithered = enable;

        if CURRENT_DEVICE.mark() < 7 {
            if enable {
                self.transform = transform_dither_g16;
            } else {
                self.transform = transform_identity;
            }
        }
    }

    fn dithered(&self) -> bool {
        self.dithered
    }

    fn width(&self) -> u32 {
        self.fb.var_screen_info.xres
    }

    fn height(&self) -> u32 {
        self.fb.var_screen_info.yres
    }

}

const DITHER_PITCH: u32 = 128; // Copied from ./kobo.rs

// The input color is in {0 .. 255}.
// The output color is in G16.
// G16 := {17 * i | i ∈ {0 .. 15}}.
fn transform_dither_g16(x: u32, y: u32, color: u8) -> u8 {
    // Get the address of the drift value.
    let addr = (x % DITHER_PITCH) + (y % DITHER_PITCH) * DITHER_PITCH;
    // Apply the drift to the input color.
    let c = (color as i16 + super::transform::DITHER_G16_DRIFTS[addr as usize] as i16).max(0).min(255);
    // Compute the distance to the previous color in G16.
    let d = c % 17;
    // Return the nearest color in G16.
    if d < 9 {
        (c - d) as u8
    } else {
        (c + (17 - d)) as u8
    }
}

fn transform_identity(_x: u32, _y: u32, color: u8) -> u8 {
    color
}

impl Drop for RemarkableFramebuffer {
    fn drop(&mut self) {
        // Framebuffer from libremarkable doesn't seem to need any cleanup
    }
}
