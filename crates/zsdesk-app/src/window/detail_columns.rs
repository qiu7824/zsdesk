//! Edit the property columns actually supported by the current Shell folder.
use super::*;
use crate::report_list;
use windows::{
    core::Interface,
    Win32::{
        Foundation::PROPERTYKEY,
        UI::Shell::{
            IColumnManager, CM_COLUMNINFO, CM_ENUM_ALL, CM_ENUM_FLAGS, CM_ENUM_VISIBLE,
            CM_MASK_NAME, CM_MASK_STATE, CM_MASK_WIDTH, CM_STATE_ALWAYSVISIBLE,
            CM_STATE_FIXEDWIDTH,
        },
    },
};

struct Column {
    key: PROPERTYKEY,
    name: String,
    visible: bool,
    required: bool,
    width: u32,
    edited_width: Option<u32>,
    fixed: bool,
}
struct Dialog {
    columns: Vec<Column>,
    active: Option<usize>,
    dark: bool,
}

fn keys(manager: &IColumnManager, which: CM_ENUM_FLAGS) -> Result<Vec<PROPERTYKEY>> {
    let count = unsafe { manager.GetColumnCount(which) }? as usize;
    anyhow::ensure!(
        count <= 8192,
        "{}",
        zsdesk_core::i18n::text("详细信息列数量无效")
    );
    let mut keys = vec![PROPERTYKEY::default(); count];
    if count > 0 {
        unsafe { manager.GetColumns(which, &mut keys) }?;
    }
    Ok(keys)
}

pub(super) fn choose(owner: HWND, browser: &ExplorerBrowserHost, dark: bool) -> Result<bool> {
    let manager: IColumnManager = unsafe { browser.current_view() }?.cast()?;
    let visible = keys(&manager, CM_ENUM_VISIBLE)?;
    let mut order = visible.clone();
    order.extend(
        keys(&manager, CM_ENUM_ALL)?
            .into_iter()
            .filter(|key| !visible.contains(key)),
    );
    let mut columns = Vec::with_capacity(order.len());
    for key in order {
        let mut info = CM_COLUMNINFO {
            cbSize: size_of::<CM_COLUMNINFO>() as u32,
            dwMask: (CM_MASK_WIDTH | CM_MASK_NAME | CM_MASK_STATE).0 as u32,
            ..Default::default()
        };
        if let Err(error) = unsafe { manager.GetColumnInfo(&key, &mut info) } {
            if visible.contains(&key) {
                return Err(error.into());
            }
            continue;
        }
        let name = String::from_utf16_lossy(
            &info.wszName[..info
                .wszName
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(info.wszName.len())],
        );
        columns.push(Column {
            key,
            name,
            visible: visible.contains(&key),
            required: info.dwState & CM_STATE_ALWAYSVISIBLE.0 as u32 != 0
                || key == zsdesk_windows::shell::commands::PKEY_NAME,
            width: info.uWidth,
            edited_width: None,
            fixed: info.dwState & CM_STATE_FIXEDWIDTH.0 as u32 != 0,
        });
    }
    let state = RefCell::new(Dialog {
        columns,
        active: None,
        dark,
    });
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }?.0);
    let answer = unsafe {
        DialogBoxParamW(
            Some(instance),
            PCWSTR(230 as *const u16),
            Some(owner),
            Some(procedure),
            LPARAM(&state as *const _ as isize),
        )
    };
    anyhow::ensure!(answer != -1, "{}", windows::core::Error::from_thread());
    if answer != 1 {
        return Ok(false);
    }
    let columns = state.into_inner().columns;
    let wanted: Vec<_> = columns
        .iter()
        .filter(|c| c.visible || c.required)
        .map(|c| c.key)
        .collect();
    if wanted != visible {
        unsafe { manager.SetColumns(&wanted) }?;
    }
    for column in columns.into_iter().filter(|c| c.visible || c.required) {
        if let Some(width) = column.edited_width {
            let info = CM_COLUMNINFO {
                cbSize: size_of::<CM_COLUMNINFO>() as u32,
                dwMask: CM_MASK_WIDTH.0 as u32,
                uWidth: width,
                ..Default::default()
            };
            unsafe { manager.SetColumnInfo(&column.key, &info) }?;
        }
    }
    Ok(true)
}

fn check(list: HWND, row: usize, value: bool) {
    let item = LVITEMW {
        stateMask: LVIS_STATEIMAGEMASK,
        state: LIST_VIEW_ITEM_STATE_FLAGS(if value { 0x2000 } else { 0x1000 }),
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            list,
            LVM_SETITEMSTATE,
            Some(WPARAM(row)),
            Some(LPARAM(&item as *const _ as isize)),
        );
    }
}
fn checked(list: HWND, row: usize) -> bool {
    unsafe {
        SendMessageW(
            list,
            LVM_GETITEMSTATE,
            Some(WPARAM(row)),
            Some(LPARAM(LVIS_STATEIMAGEMASK.0 as isize)),
        )
    }
    .0 == 0x2000
}
fn set_text(window: HWND, text: &str) {
    let _ = unsafe { SetWindowTextW(window, PCWSTR(wide(text).as_ptr())) };
}
fn select(hwnd: HWND, list: HWND, state: &mut Dialog, row: usize) {
    let Some(column) = state.columns.get(row) else {
        return;
    };
    state.active = Some(row);
    let edit = unsafe { GetDlgItem(Some(hwnd), 236) }.unwrap_or_default();
    let width = column.edited_width.unwrap_or(column.width);
    set_text(
        edit,
        &if width > 0 && width <= 32767 {
            width.to_string()
        } else {
            String::new()
        },
    );
    unsafe {
        let _ = EnableWindow(edit, !column.fixed);
        if let Ok(ok) = GetDlgItem(Some(hwnd), 1) {
            let _ = EnableWindow(ok, true);
        }
        for (id, enabled) in [(233, row > 0), (234, row + 1 < state.columns.len())] {
            if let Ok(button) = GetDlgItem(Some(hwnd), id) {
                let _ = EnableWindow(button, enabled);
            }
        }
        SendMessageW(list, LVM_ENSUREVISIBLE, Some(WPARAM(row)), None);
    }
}
fn populate(list: HWND, state: &Dialog) {
    unsafe {
        SendMessageW(list, WM_SETREDRAW, Some(WPARAM(0)), None);
    }
    report_list::clear(list);
    for (i, column) in state.columns.iter().enumerate() {
        report_list::cell(list, i, 0, &column.name);
        check(list, i, column.visible || column.required);
    }
    unsafe {
        SendMessageW(list, WM_SETREDRAW, Some(WPARAM(1)), None);
        let _ = InvalidateRect(Some(list), None, false);
    }
}
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    if msg == WM_INITDIALOG {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, lp.0);
        }
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const RefCell<Dialog>;
    if ptr.is_null() {
        return 0;
    }
    let Ok(mut state) = unsafe { &*ptr }.try_borrow_mut() else {
        return 0;
    };
    let Ok(list) = (unsafe { GetDlgItem(Some(hwnd), 232) }) else {
        return 0;
    };
    match msg {
        WM_INITDIALOG => {
            set_text(hwnd, zsdesk_core::i18n::text("选择详细信息"));
            for (id, label) in [
                (231, "显示的详细信息"),
                (233, "上移"),
                (234, "下移"),
                (235, "选中列的宽度（像素）"),
                (1, "确定"),
                (2, "取消"),
            ] {
                if let Ok(child) = unsafe { GetDlgItem(Some(hwnd), id) } {
                    set_text(child, zsdesk_core::i18n::text(label));
                }
            }
            zsdesk_windows::theme::set_dark_frame(hwnd, state.dark);
            zsdesk_windows::theme::theme_control(list, state.dark);
            for id in [1, 2, 233, 234, 236] {
                if let Ok(child) = unsafe { GetDlgItem(Some(hwnd), id) } {
                    zsdesk_windows::theme::theme_control(child, state.dark);
                }
            }
            if state.dark {
                report_list::style(list, COLORREF(0x202020), COLORREF(0xf3f3f3));
            }
            let mut r = RECT::default();
            let _ = unsafe { GetClientRect(list, &mut r) };
            report_list::heading(
                list,
                0,
                "",
                r.right - unsafe { GetSystemMetrics(SM_CXVSCROLL) },
                true,
            );
            unsafe {
                SendMessageW(
                    list,
                    LVM_SETEXTENDEDLISTVIEWSTYLE,
                    None,
                    Some(LPARAM(
                        (LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize,
                    )),
                );
            }
            populate(list, &state);
            if !state.columns.is_empty() {
                report_list::select(list, 0);
                select(hwnd, list, &mut state, 0);
            }
            1
        }
        WM_NOTIFY if lp.0 != 0 => {
            let header = unsafe { &*(lp.0 as *const NMHDR) };
            if header.hwndFrom == list && header.code == LVN_ITEMCHANGED {
                let change = unsafe { &*(lp.0 as *const NMLISTVIEW) };
                if let Ok(row) = usize::try_from(change.iItem) {
                    if let Some(c) = state.columns.get_mut(row) {
                        c.visible = checked(list, row) || c.required;
                        if c.required && !checked(list, row) {
                            check(list, row, true);
                        }
                    }
                    if change.uNewState & LVIS_SELECTED.0 != 0 && state.active != Some(row) {
                        select(hwnd, list, &mut state, row);
                    }
                }
            }
            0
        }
        WM_COMMAND => {
            match wp.0 & 0xffff {
                1 | 2 => {
                    let _ = unsafe { EndDialog(hwnd, (wp.0 & 0xffff) as isize) };
                }
                233 | 234 => {
                    if let Some(row) = state.active {
                        let other = if wp.0 & 0xffff == 233 {
                            row.checked_sub(1)
                        } else {
                            row.checked_add(1)
                        };
                        if let Some(other) = other.filter(|i| *i < state.columns.len()) {
                            state.columns.swap(row, other);
                            populate(list, &state);
                            report_list::select(list, other);
                            select(hwnd, list, &mut state, other);
                        }
                    }
                }
                236 if wp.0 >> 16 == EN_CHANGE as usize => {
                    if let Some(row) = state.active {
                        let edit = unsafe { GetDlgItem(Some(hwnd), 236) }.unwrap_or_default();
                        let value = navigation::window_text(edit);
                        let mut width = value
                            .parse::<u32>()
                            .ok()
                            .filter(|v| (1..=32767).contains(v));
                        if let Ok(ok) = unsafe { GetDlgItem(Some(hwnd), 1) } {
                            let _ =
                                unsafe { EnableWindow(ok, value.is_empty() || width.is_some()) };
                        }
                        if value.is_empty() {
                            width = None;
                        }
                        state.columns[row].edited_width = width;
                    }
                }
                _ => return 0,
            }
            1
        }
        WM_CTLCOLORDLG | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT if state.dark => {
            use windows::Win32::Graphics::Gdi::{GetStockObject, SetDCBrushColor, DC_BRUSH};
            let dc = HDC(wp.0 as *mut _);
            let color = COLORREF(if msg == WM_CTLCOLOREDIT {
                0x383838
            } else {
                0x202020
            });
            unsafe {
                SetTextColor(dc, COLORREF(0xf3f3f3));
                SetBkColor(dc, color);
                SetDCBrushColor(dc, color);
                GetStockObject(DC_BRUSH).0 as isize
            }
        }
        WM_CLOSE => {
            let _ = unsafe { EndDialog(hwnd, 2) };
            1
        }
        _ => 0,
    }
}
