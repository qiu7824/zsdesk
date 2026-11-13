use windows::{core::{Interface,IUnknown,Error,HRESULT,Result},Win32::{Foundation::{HWND,LPARAM,POINT,RECT},Graphics::Gdi::ScreenToClient,
    System::{Com::IDispatch,Variant::VARIANT},UI::{Accessibility::*,WindowsAndMessaging::*}}};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ViewMenuTarget{CurrentSelection,Selection,Background,NativeControl,Unresolved}

fn role_target(role:u32)->Option<ViewMenuTarget>{
    match role{
        ROLE_SYSTEM_COLUMNHEADER|ROLE_SYSTEM_ROWHEADER|ROLE_SYSTEM_GROUPING=>Some(ViewMenuTarget::NativeControl),
        ROLE_SYSTEM_LISTITEM|ROLE_SYSTEM_OUTLINEITEM|ROLE_SYSTEM_ROW=>Some(ViewMenuTarget::Selection),
        ROLE_SYSTEM_LIST|ROLE_SYSTEM_OUTLINE=>Some(ViewMenuTarget::Background),
        _=>None,
    }
}

/// Query the source control, so occlusion and other windows cannot change the
/// target. MSAA identifies file rows separately from headers and empty space.
/// https://learn.microsoft.com/windows/win32/api/oleacc/nf-oleacc-accessibleobjectfromwindow
struct Hit{target:ViewMenuTarget,object:Option<IAccessible>,child:VARIANT}
impl Hit{fn plain(target:ViewMenuTarget)->Self{Self{target,object:None,child:VARIANT::from(0i32)}}}
fn locate(view:HWND,_source:HWND,position:LPARAM)->Hit{
    if position.0==-1{return Hit::plain(ViewMenuTarget::CurrentSelection);}
    let point=POINT{x:position.0 as i16 as i32,y:(position.0>>16)as i16 as i32};
    let mut rect=RECT::default();
    if unsafe{GetWindowRect(view,&mut rect)}.is_err()||point.x<rect.left||point.x>=rect.right||point.y<rect.top||point.y>=rect.bottom{
        return Hit::plain(ViewMenuTarget::NativeControl);
    }
    // WM_CONTEXTMENU is often forwarded with DefView itself as its source.
    // Its accessibility object can report the whole list for a point on a file.
    // Resolve the innermost native child first, without global/occluded hits.
    let mut source=view;
    for _ in 0..32{
        let mut client=point;if !unsafe{ScreenToClient(source,&mut client)}.as_bool(){break;}
        let child=unsafe{ChildWindowFromPointEx(source,client,CWP_SKIPINVISIBLE|CWP_SKIPDISABLED)};
        if child.0.is_null()||child==source||!unsafe{IsChild(view,child)}.as_bool(){break;}source=child;
    }
    let mut raw=std::ptr::null_mut();
    if unsafe{AccessibleObjectFromWindow(source,OBJID_CLIENT.0 as u32,&IAccessible::IID,&mut raw)}.is_err()||raw.is_null(){return Hit::plain(ViewMenuTarget::Unresolved);}
    let mut object=unsafe{IAccessible::from_raw(raw)};
    let mut child=VARIANT::from(0i32);
    for _ in 0..8{
        let Ok(hit)=(unsafe{object.accHitTest(point.x,point.y)})else{break;};
        if let Ok(next)=IDispatch::try_from(&hit).and_then(|value|value.cast::<IAccessible>()){
            let unchanged=object.cast::<IUnknown>().ok()==next.cast::<IUnknown>().ok();
            if unchanged{break;}object=next;child=VARIANT::from(0i32);
        }else{child=hit;break;}
    }
    for _ in 0..8{
        if let Ok(role)=unsafe{object.get_accRole(&child)}.and_then(|value|u32::try_from(&value)){
            if let Some(target)=role_target(role){return Hit{target,object:Some(object),child};}
        }
        // An editable filename/metadata cell belongs to its item; a native
        // Edit HWND was already excluded by the view subclass.
        let Ok(parent)=unsafe{object.accParent()}.and_then(|value|value.cast::<IAccessible>())else{break;};
        if object.cast::<IUnknown>().ok()==parent.cast::<IUnknown>().ok(){break;}
        object=parent;child=VARIANT::from(0i32);
    }
    // Do not substitute an unrelated previous selection, or load a second
    // synchronous Shell menu, when accessibility cannot identify the target.
    Hit::plain(ViewMenuTarget::Unresolved)
}

pub(super) fn classify(view:HWND,source:HWND,position:LPARAM)->ViewMenuTarget{locate(view,source,position).target}

/// Mouse menus bind to the clicked item. Preserve an existing multi-selection
/// only when that item already belongs to it; keyboard menus use the selection.
pub(super) fn prepare(view:HWND,source:HWND,position:LPARAM)->Result<ViewMenuTarget>{
    let hit=locate(view,source,position);
    if hit.target!=ViewMenuTarget::Selection{return Ok(hit.target);}
    let object=hit.object.ok_or_else(||Error::from_hresult(HRESULT(0x80004005u32 as i32)))?;
    let state=unsafe{object.get_accState(&hit.child)}.and_then(|value|u32::try_from(&value))?;
    if state&STATE_SYSTEM_SELECTED==0{
        unsafe{object.accSelect((SELFLAG_TAKESELECTION|SELFLAG_TAKEFOCUS)as i32,&hit.child)}?;
        let selected=unsafe{object.get_accState(&hit.child)}.and_then(|value|u32::try_from(&value))?;
        if selected&STATE_SYSTEM_SELECTED==0{return Err(Error::new(HRESULT(0x800704c7u32 as i32),"点击的文件尚未选中，请重试"));}
    }
    Ok(hit.target)
}
