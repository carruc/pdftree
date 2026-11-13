use crate::model::Rect;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::NonNull;
use std::sync::Arc;

unsafe extern "C" {
    fn pdf_open(data: *const u8, length: usize) -> *mut c_void;
    fn pdf_count(doc: *mut c_void) -> i32;
    fn pdf_release(object: *mut c_void);
    fn pdf_page(doc: *mut c_void, index: i32, size: *mut f64) -> *mut c_void;
    fn pdf_page_text(page: *mut c_void) -> *mut c_char;
    fn pdf_text(page: *mut c_void, count: *mut u32) -> *mut [f64; 4];
    fn pdf_free(data: *mut c_void);
    fn pdf_render(page: *mut c_void, rotation: i32, width: i32, height: i32, data: *mut u8) -> i32;
}

struct Object(NonNull<c_void>);

impl Drop for Object {
    fn drop(&mut self) {
        unsafe { pdf_release(self.0.as_ptr()) }
    }
}

pub struct Document {
    object: Object,
    bytes: Arc<[u8]>,
}

impl Document {
    pub fn open(bytes: Arc<[u8]>) -> Result<Self, String> {
        let object = NonNull::new(unsafe { pdf_open(bytes.as_ptr(), bytes.len()) })
            .ok_or("Could not open PDF. It may be damaged or password protected.")?;
        Ok(Self {
            object: Object(object),
            bytes,
        })
    }

    pub fn matches(&self, bytes: &Arc<[u8]>) -> bool {
        Arc::ptr_eq(&self.bytes, bytes)
    }

    fn page(&self, index: u32) -> Result<(Object, [f32; 2]), String> {
        let mut size = [0.0; 2];
        let ptr = unsafe { pdf_page(self.object.0.as_ptr(), index as i32, size.as_mut_ptr()) };
        let page = Object(NonNull::new(ptr).ok_or("PDF page is missing.")?);
        let size = size.map(|v| v as f32);
        if size.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err("PDF page has invalid dimensions.".into());
        }
        Ok((page, size))
    }

    pub fn sizes(&self) -> Result<Vec<[f32; 2]>, String> {
        let count = unsafe { pdf_count(self.object.0.as_ptr()) };
        if count <= 0 {
            return Err("The PDF has no pages.".into());
        }
        (0..count as u32)
            .map(|i| self.page(i).map(|(_, size)| size))
            .collect()
    }

    pub fn title(&self, index: u32) -> Result<Option<String>, String> {
        let (page, _) = self.page(index)?;
        let text = unsafe { pdf_page_text(page.0.as_ptr()) };
        let Some(text) = NonNull::new(text) else {
            return Ok(None);
        };
        let title = unsafe { CStr::from_ptr(text.as_ptr()) }
            .to_string_lossy()
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned);
        unsafe { pdf_free(text.as_ptr().cast()) };
        Ok(title)
    }

    pub fn render(&self, index: u32, rotation: u8, edge: u32) -> Result<Preview, String> {
        let (page, [w, h]) = self.page(index)?;
        let [rw, rh] = if rotation % 2 == 1 { [h, w] } else { [w, h] };
        let scale = edge as f32 / rw.max(rh);
        let width = (rw * scale).round().max(1.0) as u32;
        let height = (rh * scale).round().max(1.0) as u32;
        let mut rgba = vec![0; (width * height * 4) as usize];
        let ptr = page.0.as_ptr();
        if unsafe {
            pdf_render(
                ptr,
                rotation.into(),
                width as i32,
                height as i32,
                rgba.as_mut_ptr(),
            )
        } == 0
        {
            return Err("Could not render PDF page.".into());
        }
        let mut count = 0;
        let rects = unsafe { pdf_text(ptr, &mut count) };
        let text = if rects.is_null() {
            Vec::new()
        } else {
            let text = unsafe { std::slice::from_raw_parts(rects, count as usize) }
                .iter()
                .map(|&[x, y, r, b]| {
                    let [x, y, r, b] = [x as f32 / w, y as f32 / h, r as f32 / w, b as f32 / h]
                        .map(|v| v.clamp(0.0, 1.0));
                    [x, y, (r - x).max(0.0), (b - y).max(0.0)]
                })
                .collect();
            unsafe { pdf_free(rects.cast()) };
            text
        };
        Ok(Preview {
            width,
            height,
            rgba,
            text,
        })
    }
}

pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub text: Vec<Rect>,
}
