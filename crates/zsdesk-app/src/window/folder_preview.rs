use std::path::Path;
use windows::Win32::{
        Foundation::{COLORREF, HWND, LPARAM, WPARAM},
        Graphics::Gdi::HFONT,
        UI::{Controls::*, WindowsAndMessaging::*},
};

pub(super) fn create(
    owner: HWND,
    path: &Path,
    font: HFONT,
    background: u32,
    foreground: u32,
    cancel: impl Fn() -> bool,
) -> anyhow::Result<(HWND, usize, usize)> {
    use super::folder_contents::{entries, Visibility};
    let (items, count) = entries(path, Visibility::current(), 500, cancel)?;
    unsafe {
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        })
        .ok()?;
    }
    let list = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WC_LISTVIEWW,
            None,
            WS_CHILD
                | WS_VISIBLE
                | WS_VSCROLL
                | WINDOW_STYLE(
                    LVS_REPORT | LVS_NOCOLUMNHEADER | LVS_SINGLESEL | LVS_SHAREIMAGELISTS,
                ),
            0,
            0,
            1,
            1,
            Some(owner),
            None,
            None,
            None,
        )
    }?;
    crate::report_list::style(list, COLORREF(background), COLORREF(foreground));
    crate::report_list::heading(list, 0, "", 320, true);
    unsafe {
        SendMessageW(
            list,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(0)),
        );
    }
    let mut attached = 0;
    for (row, entry) in items.iter().enumerate() {
        crate::report_list::cell(list, row, 0, &entry.name.to_string_lossy());
        if let Some(icon) = entry.icon {
            if attached != icon.images {
                unsafe { SendMessageW(list,LVM_SETIMAGELIST,Some(WPARAM(LVSIL_SMALL as usize)),Some(LPARAM(icon.images))); }
                attached = icon.images;
            }
            let item = LVITEMW { mask: LVIF_IMAGE, iItem: row as i32, iImage: icon.index, ..Default::default() };
            unsafe { SendMessageW(list,LVM_SETITEMW,None,Some(LPARAM(&item as *const _ as isize))); }
        }
    }
    Ok((list, count, items.len()))
}
