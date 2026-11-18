//! Screen capture of the Roblox client area (GDI), resized + JPEG-encoded for Claude.

use super::Rect;
use anyhow::{bail, Result};
#[allow(unused_imports)]
use image::{imageops::FilterType, Rgb, RgbImage};

pub struct Shot {
    pub jpeg: Vec<u8>,
    pub full: RgbImage,
    pub width: u32,
    pub height: u32,
    /// screen pixels per screenshot pixel
    pub scale: f64,
    pub rect: Rect,
    pub brightness: f64,
}

#[cfg(windows)]
pub fn grab_rect(r: Rect) -> Result<RgbImage> {
    use windows_sys::Win32::Graphics::Gdi::*;
    if r.width <= 0 || r.height <= 0 {
        bail!("empty capture area");
    }
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            bail!("GetDC failed");
        }
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, r.width, r.height);
        let old = SelectObject(mem, bmp as HGDIOBJ);
        let ok = BitBlt(
            mem,
            0,
            0,
            r.width,
            r.height,
            screen,
            r.left,
            r.top,
            SRCCOPY | CAPTUREBLT,
        );
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = r.width;
        bi.bmiHeader.biHeight = -r.height; // top-down
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        let mut buf = vec![0u8; (r.width * r.height * 4) as usize];
        let lines = GetDIBits(
            mem,
            bmp,
            0,
            r.height as u32,
            buf.as_mut_ptr() as *mut _,
            &mut bi,
            DIB_RGB_COLORS,
        );
        SelectObject(mem, old);
        DeleteObject(bmp as HGDIOBJ);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        if ok == 0 || lines == 0 {
            bail!("screen capture failed (PC locked?)");
        }
        let mut img = RgbImage::new(r.width as u32, r.height as u32);
        for (i, px) in img.pixels_mut().enumerate() {
            let o = i * 4;
            *px = Rgb([buf[o + 2], buf[o + 1], buf[o]]);
        }
        Ok(img)
    }
}

#[cfg(not(windows))]
pub fn grab_rect(_r: Rect) -> Result<RgbImage> {
    bail!("screen capture only works on Windows")
}

pub fn grab(r: Rect, out_width: u32, quality: u8, grid: bool) -> Result<Shot> {
    Ok(process(grab_rect(r)?, r, out_width, quality, grid))
}

pub fn process(full: RgbImage, rect: Rect, out_width: u32, quality: u8, grid: bool) -> Shot {
    let (mut small, scale) = if full.width() > out_width {
        let s = full.width() as f64 / out_width as f64;
        (
            image::imageops::resize(
                &full,
                out_width,
                (full.height() as f64 / s).round() as u32,
                FilterType::Triangle,
            ),
            s,
        )
    } else {
        (full.clone(), 1.0)
    };
    if grid {
        draw_grid(&mut small, 100);
    }
    let thumb = image::imageops::resize(&small, 64, 36, FilterType::Triangle);
    let brightness = thumb
        .pixels()
        .map(|p| 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64)
        .sum::<f64>()
        / (64.0 * 36.0);
    Shot {
        jpeg: jpeg(&small, quality),
        width: small.width(),
        height: small.height(),
        full,
        scale,
        rect,
        brightness,
    }
}

pub fn jpeg(img: &RgbImage, quality: u8) -> Vec<u8> {
    let mut out = vec![];
    let mut enc =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(30, 95));
    let _ = enc.encode_image(img);
    out
}

// 3x5 digit font for grid labels
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

fn blend(img: &mut RgbImage, x: u32, y: u32, c: [u8; 3], a: f32) {
    if x < img.width() && y < img.height() {
        let p = img.get_pixel_mut(x, y);
        for i in 0..3 {
            p[i] = (p[i] as f32 * (1.0 - a) + c[i] as f32 * a) as u8;
        }
    }
}

fn draw_num(img: &mut RgbImage, x: u32, y: u32, n: u32) {
    let s = n.to_string();
    for (ci, ch) in s.chars().enumerate() {
        let d = ch.to_digit(10).unwrap() as usize;
        for (row, bits) in DIGITS[d].iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    for sy in 0..2 {
                        for sx in 0..2 {
                            blend(
                                img,
                                x + (ci as u32 * 4 + col) * 2 + sx,
                                y + row as u32 * 2 + sy,
                                [255, 0, 255],
                                0.9,
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Faint labelled coordinate grid so Claude can pick click targets precisely.
pub fn draw_grid(img: &mut RgbImage, step: u32) {
    let (w, h) = (img.width(), img.height());
    for x in (step..w).step_by(step as usize) {
        for y in 0..h {
            blend(img, x, y, [255, 0, 255], 0.3);
        }
        draw_num(img, x + 3, 3, x);
    }
    for y in (step..h).step_by(step as usize) {
        for x in 0..w {
            blend(img, x, y, [255, 0, 255], 0.3);
        }
        draw_num(img, 3, y + 3, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_resizes_and_scales() {
        let img = RgbImage::from_pixel(2560, 1440, Rgb([200, 200, 200]));
        let s = process(
            img,
            Rect {
                left: 10,
                top: 20,
                width: 2560,
                height: 1440,
            },
            1280,
            70,
            true,
        );
        assert_eq!((s.width, s.height), (1280, 720));
        assert!((s.scale - 2.0).abs() < 1e-9);
        assert!(s.brightness > 150.0);
        assert!(s.jpeg.starts_with(&[0xFF, 0xD8]));
    }
}
