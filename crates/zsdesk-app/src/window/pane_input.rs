//! Bounded input tickets bind delayed actions to a specific pane and tab.
use super::*;
use std::sync::{Mutex,OnceLock,atomic::{AtomicUsize,Ordering}};
pub(super) const MESSAGE:u32=WM_APP+50;
const LIMIT:usize=128;
static NEXT:AtomicUsize=AtomicUsize::new(1);
static PENDING:OnceLock<Mutex<HashMap<usize,Envelope>>>=OnceLock::new();

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) struct Identity {pub source:usize,pub tab:u64,pub epoch:u64}
pub(super) enum Intent {Action(usize),Wheel(i32),OpenFolder(PathBuf),OpenFolders(Vec<PathBuf>),NavigateFolder(ShellLocation),SubmitAddress(String),CreatedItem(zsdesk_windows::shell::new_menu::CreatedItem),ContextCatalog(Vec<zsdesk_windows::shell::menu::MenuCatalogItem>),RenameChild(PathBuf),TransferMessage(String)}
struct Envelope {owner:isize,identity:Identity,intent:Intent}

pub(super) fn post(owner:HWND,identity:Identity,intent:Intent)->bool {
    let token=NEXT.fetch_add(1,Ordering::Relaxed).max(1);
    let pending=PENDING.get_or_init(Default::default);
    {
        let mut pending=pending.lock().unwrap_or_else(|e|e.into_inner());
        if pending.len()>=LIMIT{return false;}
        pending.insert(token,Envelope{owner:owner.0 as isize,identity,intent});
    }
    if unsafe{PostMessageW(Some(owner),MESSAGE,WPARAM(token),LPARAM(0))}.is_ok(){return true;}
    pending.lock().unwrap_or_else(|e|e.into_inner()).remove(&token);false
}

pub(super) fn discard(owner:HWND) {
    if let Some(pending)=PENDING.get(){pending.lock().unwrap_or_else(|e|e.into_inner()).retain(|_,envelope|envelope.owner!=owner.0 as isize);}
}

pub(super) fn blank_double_click(slot:&Slot,message:&MSG)->bool{
    if message.message!=WM_LBUTTONDBLCLK||text_input_window(message.hwnd){return false;}
    let captured=(||{
        let guard=slot.try_borrow().ok()?;let state=guard.as_ref()?;
        if !state.session.ui.double_click_blank_up||state.file_input_loading(message.hwnd){return None;}
        let(view,source)=state.file_view_for_input(message.hwnd)?;Some((state.hwnd,view,state.identity_for_source(source)?))
    })();
    let Some((owner,view,identity))=captured else{return false;};
    let mut point=POINT{x:message.lParam.0 as i16 as i32,y:(message.lParam.0>>16)as i16 as i32};
    if !unsafe{windows::Win32::Graphics::Gdi::ClientToScreen(message.hwnd,&mut point)}.as_bool(){return false;}
    let position=LPARAM(((point.x as u16 as u32)|((point.y as u16 as u32)<<16))as isize);
    if zsdesk_windows::shell::menu::classify_view_menu_point(view,message.hwnd,position)!=zsdesk_windows::shell::menu::ViewMenuTarget::Background{return false;}
    post(owner,identity,Intent::Action(Command::Up.id()as usize))
}

#[cfg(test)]pub(super) fn native_blank_double_click(slot:&Slot,root:&std::path::Path,pump:fn(u64))->Result<()>{
    use anyhow::ensure;
    let parent=root.join("blank-parent");let nested=parent.join("nested");std::fs::create_dir_all(&nested)?;
    std::fs::write(nested.join("item.txt"),b"blank navigation fixture")?;
    let(original,enabled,all_files)={let state=slot.borrow();let state=state.as_ref().unwrap();(state.current_location().clone(),state.session.ui.double_click_blank_up,state.session.ui.search_all_files)};
    slot.borrow_mut().as_mut().unwrap().session.ui.search_all_files=false;
    let navigate=|location:ShellLocation|{let mut state=slot.borrow_mut();let state=state.as_mut().unwrap();unsafe{state.navigate_to(location);}};
    let ready=|expected:&ShellLocation|->Result<()>{let until=std::time::Instant::now()+std::time::Duration::from_secs(10);
        loop{pump(20);if slot.borrow().as_ref().is_some_and(|state|state.pending.is_none()&&state.queued.is_none()&&state.current_location()==expected){break;}
            if std::time::Instant::now()>=until{let state=slot.borrow();let state=state.as_ref().unwrap();anyhow::bail!("blank-click navigation did not settle: expected={}, actual={}, pending={:?}, queued={:?}, status={}",expected.address(),state.current_location().address(),state.pending.as_ref().map(|request|request.tab.current().address()),state.queued.as_ref().map(|request|request.tab.current().address()),state.status);}}Ok(())};
    for flag in [false,true]{
        let target=ShellLocation::Path(nested.clone());navigate(target.clone());ready(&target)?;
        ensure!(slot.borrow().as_ref().unwrap().search_controls().scope.as_deref()==Some(nested.as_path()),"folder search still points to a previous directory");
        let view={let mut state=slot.borrow_mut();let state=state.as_mut().unwrap();state.session.ui.double_click_blank_up=flag;state.view_hwnd};
        let mut rect=RECT::default();unsafe{GetClientRect(view,&mut rect)?;}
        let point=POINT{x:rect.right-30,y:rect.bottom-30};
        let message=MSG{hwnd:view,message:WM_LBUTTONDBLCLK,lParam:LPARAM(((point.x as u16 as u32)|((point.y as u16 as u32)<<16))as isize),..Default::default()};
        ensure!(blank_double_click(slot,&message)==flag,"blank double-click setting not honored");
        if flag{ready(&ShellLocation::Path(parent.clone()))?;ensure!(slot.borrow().as_ref().unwrap().search_controls().scope.as_deref()==Some(parent.as_path()),"folder search scope did not follow parent navigation");}else{pump(50);ensure!(slot.borrow().as_ref().unwrap().current_location()==&target,"disabled double-click navigated");}
    }
    {let mut state=slot.borrow_mut();let state=state.as_mut().unwrap();state.session.ui.double_click_blank_up=enabled;state.session.ui.search_all_files=all_files;}
    navigate(original.clone());ready(&original)?;Ok(())
}

pub(super) fn identity(owner:HWND,source:usize)->Option<Identity> {
    let pointer=unsafe{GetWindowLongPtrW(owner,GWLP_USERDATA)}as *const Slot;
    if pointer.is_null(){return None;}
    unsafe{&*pointer}.try_borrow().ok().and_then(|slot|slot.as_ref().and_then(|state|state.identity_for_source(source)))
}

/// A single drop is one ticket: opening its first tab must not invalidate the
/// other paths from that same drop. Identity is captured before the OLE call.
pub(super) fn post_tab_paths(owner:HWND,identity:Identity,paths:Vec<PathBuf>)->bool{
    if paths.is_empty()||paths.len()>32{return false;}
    post(owner,identity,Intent::OpenFolders(paths))
}

fn take(owner:HWND,token:usize)->Option<Envelope>{
    let mut pending=PENDING.get()?.lock().unwrap_or_else(|e|e.into_inner());
    if pending.get(&token)?.owner!=owner.0 as isize{return None;}
    pending.remove(&token)
}

pub(super) fn capture_folder_open(owner:HWND,source:usize,view:HWND)->Option<zsdesk_windows::shell::FolderOpenCallback> {
    let pointer=unsafe{GetWindowLongPtrW(owner,GWLP_USERDATA)}as *const Slot;
    if pointer.is_null(){return None;}
    let snapshot={let slot=unsafe{&*pointer}.try_borrow().ok()?;let state=slot.as_ref()?;
        if state.source_for_view(view)!=Some(source){return None;}
        state.identity_for_source(source)?
    };
    Some(Box::new(move|location|{
        if !post(owner,snapshot,Intent::NavigateFolder(location)) {
            diagnostics::record("navigation","native-open-mailbox-full",Some(0x800700aau32 as i32));
        }
    }))
}

impl WindowState {
    pub(super) fn identity_for_source(&self,source:usize)->Option<Identity> {
        if source==0{return None;}
        if source==self.browser_source {
            return self.session.tabs.get(self.session.active_tab).map(|tab|Identity{source,tab:tab.id,epoch:self.navigation_epoch});
        }
        if self.session.ui.dual_pane && source==self.other_source() {
            return self.other_identity();
        }
        None
    }

    pub(super) fn queue_pane_action(&self,source:usize,action:usize)->bool {
        self.identity_for_source(source).is_some_and(|identity|post(self.hwnd,identity,Intent::Action(action)))
    }

    pub(super) fn receive_pane_input(&mut self,token:usize) {
        let Some(envelope)=take(self.hwnd,token)else{return;};
        if self.identity_for_source(envelope.identity.source)!=Some(envelope.identity){return;}
        match envelope.intent {
            Intent::TransferMessage(message)=>self.show_message(message),
            Intent::RenameChild(path)=>{
                if envelope.identity.source!=self.browser_source{self.activate_pane(!self.active_pane_right);}
                // Activating the validated source pane advances its epoch;
                // retain the exact source/tab after that deliberate switch.
                if envelope.identity.source!=self.browser_source||self.session.active_tab().id!=envelope.identity.tab{return;}
                let (Some(parent),Some(name))=(path.parent(),path.file_name())else{return;};
                let location=ShellLocation::Path(parent.into());
                if !shell_registration::same_location(self.current_location(),&location){return;}
                self.defer_selection(vec![name.to_os_string()],true);
            },
            Intent::ContextCatalog(catalog)=>self.manage_context_catalog(catalog),
            Intent::Action(action)=>self.pane_action(envelope.identity.source,action),
            Intent::Wheel(delta)=>self.pane_wheel(envelope.identity.source,delta),
            Intent::OpenFolder(path)=>self.pane_open_folder(envelope.identity.source,path),
            Intent::OpenFolders(paths)=>self.pane_open_folders(envelope.identity.source,paths),
            Intent::NavigateFolder(location)=>self.pane_navigate_folder(envelope.identity.source,location),
            Intent::CreatedItem(item)=>{
                if envelope.identity.source==self.browser_source{self.select_created_item(item.path,item.rename);}
                else if let (Some(parent),Some(name))=(item.path.parent(),item.path.file_name()){
                    self.select_created_in_other_pane(&ShellLocation::Path(parent.into()),name);
                }
            },
            Intent::SubmitAddress(text)=>if envelope.identity.source==self.browser_source {
                address_input::delivered(self.address_edit);
                diagnostics::record("navigation","address-submit-dispatched",None);
                unsafe{self.commit_address_text(&text);}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tab_path_ticket_is_owned_single_use_and_discarded_after_window_close(){
        let owner=unsafe{CreateWindowExW(WINDOW_EX_STYLE::default(),windows::core::w!("STATIC"),None,WS_POPUP,0,0,8,8,None,None,None,None)}.unwrap();
        let mut state=WindowState::new(ExplorerSession::default(),PathBuf::new(),None,false);state.hwnd=owner;state.browser_source=17;
        let expected=PathBuf::from(r"E:\中文 空格\folder");
        let identity=state.identity_for_source(17).unwrap();
        let batch=vec![expected.clone(),PathBuf::from(r"E:\second"),expected];
        assert!(post_tab_paths(owner,identity,batch.clone()));
        let mut message=MSG::default();
        assert!(unsafe{PeekMessageW(&mut message,Some(owner),MESSAGE,MESSAGE,PM_REMOVE)}.as_bool());
        assert_eq!(message.lParam.0,0);
        assert!(take(HWND::default(),message.wParam.0).is_none(),"another window consumed the request");
        let envelope=take(owner,message.wParam.0).unwrap();
        assert_eq!(envelope.identity.source,17);
        assert!(matches!(envelope.intent,Intent::OpenFolders(paths)if paths==batch));
        assert!(take(owner,message.wParam.0).is_none(),"duplicate delivery reused the payload");
        assert!(post_tab_paths(owner,identity,batch));
        assert!(unsafe{PeekMessageW(&mut message,Some(owner),MESSAGE,MESSAGE,PM_REMOVE)}.as_bool());
        discard(owner);assert!(take(owner,message.wParam.0).is_none());
        unsafe{let _=DestroyWindow(owner);}
    }
    #[test]
    fn stale_open_folder_ticket_does_not_change_the_current_tab(){
        let owner=unsafe{CreateWindowExW(WINDOW_EX_STYLE::default(),windows::core::w!("STATIC"),None,WS_POPUP,0,0,8,8,None,None,None,None)}.unwrap();
        let mut state=WindowState::new(ExplorerSession::default(),PathBuf::new(),None,false);state.hwnd=owner;state.browser_source=19;
        let identity=state.identity_for_source(19).unwrap();let tabs=state.session.tabs.len();
        assert!(post(owner,identity,Intent::OpenFolder(PathBuf::from(r"E:\delayed"))));
        let mut message=MSG::default();assert!(unsafe{PeekMessageW(&mut message,Some(owner),MESSAGE,MESSAGE,PM_REMOVE)}.as_bool());
        state.navigation_epoch+=1;state.receive_pane_input(message.wParam.0);state.receive_pane_input(message.wParam.0);
        assert_eq!(state.session.tabs.len(),tabs);assert!(take(owner,message.wParam.0).is_none());
        unsafe{let _=DestroyWindow(owner);}
    }
    #[test]
    fn rename_ticket_requires_the_exact_live_folder_and_epoch(){
        let owner=unsafe{CreateWindowExW(WINDOW_EX_STYLE::default(),windows::core::w!("STATIC"),None,WS_POPUP,0,0,8,8,None,None,None,None)}.unwrap();
        let mut session=ExplorerSession::default();session.tabs[0].replace_current(ShellLocation::Path(r"E:\source".into()));
        let mut state=WindowState::new(session,PathBuf::new(),None,false);state.hwnd=owner;state.browser_source=19;
        let identity=state.identity_for_source(19).unwrap();let mut message=MSG::default();
        for (path,accepted) in [(r"E:\different\target.txt",false),(r"E:\source\target.txt",true)]{
            assert!(post(owner,identity,Intent::RenameChild(path.into())));
            assert!(unsafe{PeekMessageW(&mut message,Some(owner),MESSAGE,MESSAGE,PM_REMOVE)}.as_bool());
            state.receive_pane_input(message.wParam.0);assert_eq!(state.deferred_selection.is_some(),accepted);state.deferred_selection=None;
        }
        assert!(post(owner,identity,Intent::RenameChild(r"E:\source\target.txt".into())));
        assert!(unsafe{PeekMessageW(&mut message,Some(owner),MESSAGE,MESSAGE,PM_REMOVE)}.as_bool());
        state.navigation_epoch+=1;state.receive_pane_input(message.wParam.0);assert!(state.deferred_selection.is_none());
        discard(owner);unsafe{let _=DestroyWindow(owner);}
    }
    #[test]
    fn stale_tab_and_navigation_inputs_are_rejected_without_consuming_new_state() {
        let mut state=WindowState::new(ExplorerSession::default(),PathBuf::new(),None,false);state.browser_source=17;
        let first=state.identity_for_source(17).unwrap();
        state.session.tabs.push(ExplorerTab::new(2,ShellLocation::ThisPc));state.session.active_tab=1;
        assert_ne!(state.identity_for_source(17),Some(first));
        state.session.active_tab=0;state.navigation_epoch+=1;
        assert_ne!(state.identity_for_source(17),Some(first));
        assert!(state.identity_for_source(18).is_none());
    }
}
