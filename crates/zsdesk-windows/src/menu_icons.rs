//! Monochrome icon-font glyphs for native popup menus.
//!
//! Windows 11 Explorer shows a 16-DIP Fluent glyph beside each command in its
//! menus. Native `HMENU`s draw a 32-bpp premultiplied bitmap in the same
//! gutter on Windows 10 and 11, so each glyph is rasterized once with GDI
//! grayscale antialiasing and its coverage becomes the alpha channel. Windows
//! 10 uses Segoe MDL2 Assets, which has the same code points.

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, RECT},
        Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, CreateFontIndirectW, DeleteDC, DeleteObject, DrawTextW, GdiFlush,
            GetTextFaceW, SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER,
            BI_RGB, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HBITMAP, HDC,
            HFONT, HGDIOBJ, LOGFONTW, OUT_TT_PRECIS, TRANSPARENT,
        },
        UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::{
            SetMenuInfo, SetMenuItemInfoW, HMENU, MENUINFO, MENUITEMINFOW, MIIM_BITMAP, MIM_APPLYTOSUBMENUS, MIM_STYLE,
            MNS_CHECKORBMP,
        }},
    },
};

const FACES: [&str; 2] = ["Segoe Fluent Icons", "Segoe MDL2 Assets"];

/// Glyph bitmaps for one popup menu. Keep it alive until the menu is destroyed:
/// `DestroyMenu` does not free item bitmaps, dropping this does.
pub struct MenuIcons {
    dc: HDC,
    font: Option<HFONT>,
    size: i32,
    color: (u32, u32, u32),
    bitmaps: Vec<HBITMAP>,
}

impl MenuIcons {
    /// Sized for `owner`'s DPI; `dark` follows the menu theme of this process.
    pub fn new(owner: HWND) -> Self {
        let dpi = if owner.0.is_null() { 96 } else { unsafe { GetDpiForWindow(owner) }.max(96) };
        let size = (16 * dpi as i32 + 48) / 96;
        let dark = crate::theme::menus_dark();
        let color = if dark { (0xF2, 0xF2, 0xF2) } else { (0x1A, 0x1A, 0x1A) };
        let dc = unsafe { CreateCompatibleDC(None) };
        let mut icons = Self { dc, font: None, size, color, bitmaps: Vec::new() };
        // High contrast menus use system colors that a pre-tinted glyph cannot follow.
        if !dc.is_invalid() && !crate::theme::high_contrast() { icons.font = FACES.iter().find_map(|face| icons.create_font(face)); }
        icons
    }

    fn create_font(&self, face: &str) -> Option<HFONT> {
        let mut logfont = LOGFONTW { lfHeight: -self.size, lfWeight: 400, lfCharSet: DEFAULT_CHARSET,
            lfOutPrecision: OUT_TT_PRECIS, lfQuality: ANTIALIASED_QUALITY, ..Default::default() };
        for (slot, unit) in logfont.lfFaceName.iter_mut().zip(face.encode_utf16().take(31)) { *slot = unit; }
        let font = unsafe { CreateFontIndirectW(&logfont) };
        if font.is_invalid() { return None; }
        // GDI silently substitutes a missing face; only accept the requested one.
        let previous = unsafe { SelectObject(self.dc, HGDIOBJ(font.0)) };
        let mut name = [0u16; 64];
        let length = unsafe { GetTextFaceW(self.dc, Some(&mut name)) }.max(0) as usize;
        unsafe { SelectObject(self.dc, previous); }
        let actual = String::from_utf16_lossy(&name[..length.saturating_sub(1).min(name.len())]);
        if actual.trim_end_matches('\0').eq_ignore_ascii_case(face) { return Some(font); }
        unsafe { let _ = DeleteObject(HGDIOBJ(font.0)); }
        None
    }

    /// Shows `glyph` beside item `item` (a command id, or a position when
    /// `by_position`). Disabled items get a dimmed glyph like Explorer's.
    pub fn attach(&mut self, menu: HMENU, item: u32, by_position: bool, glyph: &str, enabled: bool) -> bool {
        let Some(bitmap) = self.render(glyph, enabled) else { return false };
        let info = MENUITEMINFOW { cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_BITMAP,
            hbmpItem: bitmap, ..Default::default() };
        if unsafe { SetMenuItemInfoW(menu, item, by_position, &info) }.is_err() { return false; }
        // Checked items without a glyph keep the check mark in the shared gutter.
        let style = MENUINFO { cbSize: std::mem::size_of::<MENUINFO>() as u32, fMask: MIM_STYLE | MIM_APPLYTOSUBMENUS,
            dwStyle: MNS_CHECKORBMP, ..Default::default() };
        let _ = unsafe { SetMenuInfo(menu, &style) };
        true
    }

    fn render(&mut self, glyph: &str, enabled: bool) -> Option<HBITMAP> {
        let font = self.font?;
        let size = self.size;
        let header = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: size,
            biHeight: -size, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() };
        let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap = unsafe { CreateDIBSection(Some(self.dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
        if bits.is_null() { unsafe { let _ = DeleteObject(HGDIOBJ(bitmap.0)); } return None; }
        let count = (size * size) as usize;
        // SAFETY: a top-down 32-bpp DIB of `size`² pixels backs `bits` while `bitmap` lives.
        let pixels = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u32>(), count) };
        pixels.fill(0);
        unsafe {
            let old_bitmap = SelectObject(self.dc, HGDIOBJ(bitmap.0));
            let old_font = SelectObject(self.dc, HGDIOBJ(font.0));
            SetTextColor(self.dc, COLORREF(0x00FF_FFFF));
            SetBkMode(self.dc, TRANSPARENT);
            let mut rect = RECT { left: 0, top: 0, right: size, bottom: size };
            let mut text: Vec<u16> = glyph.encode_utf16().collect();
            DrawTextW(self.dc, &mut text, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
            let _ = GdiFlush();
            SelectObject(self.dc, old_font);
            SelectObject(self.dc, old_bitmap);
        }
        let opacity = if enabled { 255 } else { 102 };
        for pixel in pixels.iter_mut() { *pixel = tint(*pixel, self.color, opacity); }
        self.bitmaps.push(bitmap);
        Some(bitmap)
    }
}

/// White-on-black grayscale coverage → premultiplied BGRA in `color`.
fn tint(coverage: u32, (r, g, b): (u32, u32, u32), opacity: u32) -> u32 {
    let level = (coverage & 0xFF).max((coverage >> 8) & 0xFF).max((coverage >> 16) & 0xFF);
    let alpha = level * opacity / 255;
    (alpha << 24) | ((r * alpha / 255) << 16) | ((g * alpha / 255) << 8) | (b * alpha / 255)
}

impl Drop for MenuIcons {
    fn drop(&mut self) {
        unsafe {
            for bitmap in self.bitmaps.drain(..) { let _ = DeleteObject(HGDIOBJ(bitmap.0)); }
            if let Some(font) = self.font.take() { let _ = DeleteObject(HGDIOBJ(font.0)); }
            if !self.dc.is_invalid() { let _ = DeleteDC(self.dc); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tint;
    #[test]
    fn coverage_becomes_premultiplied_alpha() {
        assert_eq!(tint(0, (0x1A, 0x1A, 0x1A), 255), 0);
        assert_eq!(tint(0x00FF_FFFF, (0xF2, 0xF2, 0xF2), 255), 0xFFF2_F2F2);
        let dim = tint(0x00FF_FFFF, (0xFF, 0x00, 0x00), 102);
        assert_eq!(dim >> 24, 102);
        assert_eq!((dim >> 16) & 0xFF, 102, "color channels never exceed alpha");
    }
}
