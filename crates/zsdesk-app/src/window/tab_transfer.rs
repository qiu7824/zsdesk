use super::*;
use serde::{Deserialize, Serialize};
use windows::Win32::{
    Graphics::Gdi::{ClientToScreen, ScreenToClient},
    System::DataExchange::COPYDATASTRUCT,
};

pub(super) const MESSAGE: usize = 0x5a54_4142;
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Transfer {
    tab: ExplorerTab,
    screen: [i32; 2],
}

fn profile_window(window: HWND, profile: &std::path::Path) -> bool {
    let key = crate::instance::property(profile);
    let mut name = [0; 64];
    let count = unsafe { GetClassNameW(window, &mut name) }.max(0) as usize;
    String::from_utf16_lossy(&name[..count]) == "ZSDesk.Native.Explorer"
        && unsafe { GetPropW(window, PCWSTR(key.as_ptr())) }.0 as usize == 1
}

impl WindowState {
    // Return true for a drop on a peer even if that peer declines it:
    // a busy/closing destination must never cause a second tear-off.
    pub(super) fn merge_dragged_tab(&mut self, index: usize, mut point: POINT) -> bool {
        if !unsafe { ClientToScreen(self.hwnd, &mut point) }.as_bool() {
            return false;
        }
        let target = unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) };
        if target.is_invalid() || target == self.hwnd || !profile_window(target, &self.config_path)
        {
            return false;
        }
        if self.pending.is_some()
            || self.picker.is_some()
            || self.session.tabs.get(index).is_none_or(|tab| tab.locked)
        {
            return true;
        }
        unsafe {
            self.capture_active();
        }
        let packet = Transfer {
            tab: self.session.tabs[index].clone(),
            screen: [point.x, point.y],
        };
        let Ok(bytes) = serde_json::to_vec(&packet) else {
            return true;
        };
        if bytes.len() > MAX_BYTES {
            return true;
        }
        let data = COPYDATASTRUCT {
            dwData: MESSAGE,
            cbData: bytes.len() as u32,
            lpData: bytes.as_ptr() as *mut _,
        };
        let mut accepted = 0;
        let sent = unsafe {
            SendMessageTimeoutW(
                target,
                WM_COPYDATA,
                WPARAM(self.hwnd.0 as usize),
                LPARAM(&data as *const _ as isize),
                SMTO_BLOCK | SMTO_ABORTIFHUNG,
                1500,
                Some(&mut accepted),
            )
        };
        if sent.0 != 0 && accepted == 1 {
            unsafe {
                self.close_tab(index);
            }
            crate::instance::request_activation(target);
        }
        true
    }
}

pub(super) fn receive(hwnd: HWND, sender: HWND, data: &COPYDATASTRUCT) -> LRESULT {
    if data.lpData.is_null()
        || data.cbData == 0
        || data.cbData as usize > MAX_BYTES
        || sender == hwnd
    {
        return LRESULT(0);
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Slot;
    if pointer.is_null() {
        return LRESULT(0);
    }
    let Ok(mut slot) = unsafe { &*pointer }.try_borrow_mut() else {
        return LRESULT(0);
    };
    let Some(state) = slot.as_mut() else {
        return LRESULT(0);
    };
    if state.destroyed
        || state.picker.is_some()
        || state.pending.is_some()
        || state.queued.is_some()
        || !profile_window(sender, &state.config_path)
    {
        return LRESULT(0);
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize) };
    let Ok(mut packet) = serde_json::from_slice::<Transfer>(bytes) else {
        return LRESULT(0);
    };
    let mut point = POINT {
        x: packet.screen[0],
        y: packet.screen[1],
    };
    if packet.tab.locked || !unsafe { ScreenToClient(hwnd, &mut point) }.as_bool() {
        return LRESULT(0);
    }
    let index = match state.layout.hit(point.x, point.y) {
        Hit::Tab(i) | Hit::TabClose(i) => i,
        Hit::NewTab => state.session.tabs.len(),
        _ if state
            .layout
            .tab_strip_right_hit(point.x, point.y, true)
            .is_some() =>
        {
            state.session.tabs.len()
        }
        _ => return LRESULT(0),
    };
    packet.tab.normalize();
    packet.tab.id = state.session.take_id();
    state.session.tabs.insert(index, packet.tab);
    if index <= state.session.active_tab {
        state.session.active_tab += 1;
    }
    // Acknowledge ownership before starting potentially slow Shell navigation.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::ReplyMessage(LRESULT(1));
    }
    unsafe {
        state.select_tab(index);
    }
    state.save();
    LRESULT(1)
}
