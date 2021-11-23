use imgref::ImgVec;
use rayon::prelude::*;
use std::cell::RefCell;
use std::error::Error;
//use std::error::Error;
use std::os::raw::c_char;
use std::os::raw::c_int;
use std::ptr;
use std::fmt;
use std::slice;
use log::*;
//use clap::Format::Error;

thread_local!{
    static LAST_ERROR: RefCell<Option<Box<dyn Error>>> = RefCell::new(None);
}

type BoxError = Box<dyn Error + Send + Sync>;

use ravif::*;

#[repr(C)]
struct Buffer {
    data: *mut u8,
    len: usize,
}

pub extern "C" fn enc_rgba(data: *const c_char, dataSize: usize, config: &Config) -> *mut Buffer {
    if data.is_null() {
        let err = FfiAvifError::new("No input data pointer provided");
        update_last_error(err);
        return ptr::null_mut();
    }

    let mut buffer: &[u8] = unsafe { std::slice::from_raw_parts(data as *const u8, dataSize) };

    let mut img = match load_rgba(&buffer, false) {
        Ok(i) => i,
        Err(_) => return ptr::null_mut(),
        Err(e) => {
            update_last_error(e.unwrap());
            return ptr::null_mut();
        }
    };

    let (out_data, _, _) = match encode_rgba(img.as_ref(), config) {
        Ok(d) => d,
        Err(_) => return ptr::null_mut(),
        Err(e) => {
            update_last_error(e.into());
            return ptr::null_mut();
        }
    };

    //let mut odata = out_data.align_to_mut();

    let b = Buffer { data: out_data.as_ptr() as *mut u8, len: out_data.len()};

    &mut b
}

extern "C" fn free_buf(buf: Buffer) {
    let s = unsafe { std::slice::from_raw_parts_mut(buf.data, buf.len) };
    let s = s.as_mut_ptr();
    unsafe {
        Box::from_raw(s);
    }
}

#[cfg(not(feature = "cocoa_image"))]
fn load_rgba(mut data: &[u8], premultiplied_alpha: bool) -> Result<ImgVec<RGBA8>, Box<dyn std::error::Error + Send + Sync>> {
    use rgb::FromSlice;

    let mut img = if data.get(0..4) == Some(&[0x89,b'P',b'N',b'G']) {
        let img = lodepng::decode32(data)?;
        ImgVec::new(img.buffer, img.width, img.height)
    } else {
        let mut jecoder = jpeg_decoder::Decoder::new(&mut data);
        let pixels = jecoder.decode()?;
        let info = jecoder.info().ok_or("Error reading JPEG info")?;
        use jpeg_decoder::PixelFormat::*;
        let buf: Vec<_> = match info.pixel_format {
            L8 => {
                pixels.iter().copied().map(|g| RGBA8::new(g,g,g,255)).collect()
            },
            RGB24 => {
                let rgb = pixels.as_rgb();
                rgb.iter().map(|p| p.alpha(255)).collect()
            },
            CMYK32 => return Err("CMYK JPEG is not supported. Please convert to PNG first".into()),
        };
        ImgVec::new(buf, info.width.into(), info.height.into())
    };
    if premultiplied_alpha {
        img.pixels_mut().for_each(|px| {
            px.r = (px.r as u16 * px.a as u16 / 255) as u8;
            px.g = (px.g as u16 * px.a as u16 / 255) as u8;
            px.b = (px.b as u16 * px.a as u16 / 255) as u8;
        });
    }
    Ok(img)
}

#[cfg(feature = "cocoa_image")]
fn load_rgba(data: &[u8], premultiplied_alpha: bool) -> Result<ImgVec<RGBA8>, BoxError> {
    if premultiplied_alpha {
        Ok(cocoa_image::decode_image_as_rgba_premultiplied(data)?)
    } else {
        Ok(cocoa_image::decode_image_as_rgba(data)?)
    }
}

// Error handling

/// Calculate the number of bytes in the last error's error message **not**
/// including any trailing `null` characters.
#[no_mangle]
pub extern "C" fn last_error_length() -> c_int {
    LAST_ERROR.with(|prev| match *prev.borrow() {
        Some(ref err) => err.to_string().len() as c_int + 1,
        None => 0,
    })
}

/// Write the most recent error message into a caller-provided buffer as a UTF-8
/// string, returning the number of bytes written.
///
/// # Note
///
/// This writes a **UTF-8** string into the buffer. Windows users may need to
/// convert it to a UTF-16 "unicode" afterwards.
///
/// If there are no recent errors then this returns `0` (because we wrote 0
/// bytes). `-1` is returned if there are any errors, for example when passed a
/// null pointer or a buffer of insufficient size.
#[no_mangle]
pub unsafe extern "C" fn last_error_message(buffer: *mut c_char, length: c_int) -> c_int {
    if buffer.is_null() {
        warn!("Null pointer passed into last_error_message() as the buffer");
        return -1;
    }

    let last_error = match take_last_error() {
        Some(err) => err,
        None => return 0,
    };

    let error_message = last_error.to_string();

    let buffer = slice::from_raw_parts_mut(buffer as *mut u8, length as usize);

    if error_message.len() >= buffer.len() {
        warn!("Buffer provided for writing the last error message is too small.");
        warn!(
            "Expected at least {} bytes but got {}",
            error_message.len() + 1,
            buffer.len()
        );
        return -1;
    }

    ptr::copy_nonoverlapping(
        error_message.as_ptr(),
        buffer.as_mut_ptr(),
        error_message.len(),
    );

    // Add a trailing null so people using the string as a `char *` don't
    // accidentally read into garbage.
    buffer[error_message.len()] = 0;

    error_message.len() as c_int
}

/// Update the most recent error, clearing whatever may have been there before.
pub fn update_last_error<E: std::error::Error + 'static>(err: E) {
    error!("Setting LAST_ERROR: {}", err);

    {
        // Print a pseudo-backtrace for this error, following back each error's
        // cause until we reach the root error.
        let mut cause = err.source();
        while let Some(parent_err) = cause {
            warn!("Caused by: {}", parent_err);
            cause = parent_err.source();
        }
    }

    LAST_ERROR.with(|prev| {
        *prev.borrow_mut() = Some(Box::new(err));
    });
}

/// Retrieve the most recent error, clearing it in the process.
pub fn take_last_error() -> Option<Box<dyn std::error::Error>> {
    LAST_ERROR.with(|prev| prev.borrow_mut().take())
}

#[derive(Debug)]
pub struct FfiAvifError {
    details: String
}

impl FfiAvifError {
    fn new(msg: &str) -> FfiAvifError {
        FfiAvifError{details: msg.to_string()}
    }
}

impl fmt::Display for FfiAvifError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,"{}",self.details)
    }
}

impl std::error::Error for FfiAvifError {
    fn description(&self) -> &str {
        &self.details
    }
}