#![allow(unsafe_op_in_unsafe_fn)]

use crate::assets::MAX_IMAGE_BYTES;
use std::{io, mem::size_of, slice};
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::*,
    System::{
        DataExchange::{
            CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
            RegisterClipboardFormatW,
        },
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        Ole::CF_BITMAP,
    },
};

const BMP_HEADER: usize = 54;

struct Clipboard;

impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

pub unsafe fn image(hwnd: HWND) -> io::Result<Option<Vec<u8>>> {
    let png_name = [b'P' as u16, b'N' as u16, b'G' as u16, 0];
    let png = RegisterClipboardFormatW(png_name.as_ptr());
    let has_png = png != 0 && IsClipboardFormatAvailable(png) != 0;
    let bitmap = u32::from(CF_BITMAP);
    let has_bitmap = IsClipboardFormatAvailable(bitmap) != 0;
    if !has_png && !has_bitmap {
        return Ok(None);
    }
    if OpenClipboard(hwnd) == 0 {
        return Err(io::Error::last_os_error());
    }
    let _clipboard = Clipboard;
    if has_png {
        let handle = GetClipboardData(png);
        if !handle.is_null() {
            let length = GlobalSize(handle);
            if length as u64 > MAX_IMAGE_BYTES {
                return Err(io::Error::other("剪贴板图片超过 20 MiB"));
            }
            let pointer = GlobalLock(handle);
            if !pointer.is_null() {
                let bytes = slice::from_raw_parts(pointer.cast::<u8>(), length);
                let copy = bytes
                    .starts_with(b"\x89PNG\r\n\x1a\n")
                    .then(|| bytes.to_vec());
                GlobalUnlock(handle);
                if copy.is_some() {
                    return Ok(copy);
                }
            }
        }
    }
    let handle = GetClipboardData(bitmap);
    if handle.is_null() {
        return Err(io::Error::other("无法读取剪贴板图片"));
    }
    bitmap_bytes(hwnd, handle).map(Some)
}

unsafe fn bitmap_bytes(hwnd: HWND, handle: HBITMAP) -> io::Result<Vec<u8>> {
    let mut bitmap = BITMAP::default();
    if GetObjectW(
        handle,
        size_of::<BITMAP>() as i32,
        (&mut bitmap as *mut BITMAP).cast(),
    ) == 0
        || bitmap.bmWidth <= 0
        || bitmap.bmHeight <= 0
    {
        return Err(io::Error::other("剪贴板位图无效"));
    }
    let width = bitmap.bmWidth;
    let height = bitmap.bmHeight;
    let stride = (width as u64 * 3).next_multiple_of(4);
    let image_size = stride * height as u64;
    if image_size + BMP_HEADER as u64 > MAX_IMAGE_BYTES {
        return Err(io::Error::other("剪贴板位图超过 20 MiB"));
    }
    let mut bytes = vec![0; BMP_HEADER + image_size as usize];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: 40,
            biWidth: width,
            biHeight: height,
            biPlanes: 1,
            biBitCount: 24,
            biCompression: BI_RGB,
            biSizeImage: image_size as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let dc = GetDC(hwnd);
    if dc.is_null() {
        return Err(io::Error::last_os_error());
    }
    let rows = GetDIBits(
        dc,
        handle,
        0,
        height as u32,
        bytes[BMP_HEADER..].as_mut_ptr().cast(),
        &mut info,
        DIB_RGB_COLORS,
    );
    ReleaseDC(hwnd, dc);
    if rows != height {
        return Err(io::Error::other("无法转换剪贴板位图"));
    }
    let length = bytes.len() as u32;
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&length.to_le_bytes());
    bytes[10..14].copy_from_slice(&(BMP_HEADER as u32).to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&width.to_le_bytes());
    bytes[22..26].copy_from_slice(&height.to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&(image_size as u32).to_le_bytes());
    Ok(bytes)
}
