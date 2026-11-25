//! IPC access to the external Everything executable selected in this profile.
use std::{os::windows::process::CommandExt,path::{Path,PathBuf},process::{Command,Stdio},thread,time::{Duration,Instant}};
use anyhow::{bail,ensure,Context,Result};
use sha2::{Digest,Sha256};
use windows::{core::{PCWSTR,PWSTR},Win32::{Foundation::{CloseHandle,FILETIME,HANDLE,HWND,LPARAM,WPARAM,WAIT_ABANDONED,WAIT_OBJECT_0,WAIT_TIMEOUT},System::Threading::{CreateMutexW,GetProcessTimes,OpenProcess,QueryFullProcessImageNameW,ReleaseMutex,WaitForSingleObject,PROCESS_NAME_WIN32,PROCESS_QUERY_LIMITED_INFORMATION},UI::WindowsAndMessaging::*}};
#[path="everything_status.rs"]
mod status;
pub(super) fn status_report(profile:&Path)->String{status::report(profile)}
fn wide(value:&str)->Vec<u16>{value.encode_utf16().chain(Some(0)).collect()}
struct Handle(HANDLE);
impl Drop for Handle{fn drop(&mut self){let _=unsafe{CloseHandle(self.0)};}}
struct StartLock(Handle);
impl Drop for StartLock{fn drop(&mut self){let _=unsafe{ReleaseMutex(self.0.0)};}}
#[derive(Clone,Debug,PartialEq,Eq)]
struct Identity{pid:u32,created:u64,executable:PathBuf}
pub(super) struct Connection { pub hwnd:HWND,pub executable:PathBuf,pub instance:Option<String> }
fn identity_pid(pid:u32)->Option<Identity>{
    if pid==0{return None;}
    let process=Handle(unsafe{OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid)}.ok()?);
    let mut buffer=vec![0u16;32768];let mut length=buffer.len()as u32;
    unsafe{QueryFullProcessImageNameW(process.0,PROCESS_NAME_WIN32,PWSTR(buffer.as_mut_ptr()),&mut length)}.ok()?;
    let mut created=FILETIME::default();let mut exit=FILETIME::default();let mut kernel=FILETIME::default();let mut user=FILETIME::default();
    unsafe{GetProcessTimes(process.0,&mut created,&mut exit,&mut kernel,&mut user)}.ok()?;
    Some(Identity{pid,created:(u64::from(created.dwHighDateTime)<<32)|u64::from(created.dwLowDateTime),executable:PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]))})
}
fn identity(hwnd:HWND)->Option<Identity>{let mut pid=0;unsafe{GetWindowThreadProcessId(hwnd,Some(&mut pid));}identity_pid(pid)}
fn ordinary_everything(path:&Path)->bool{
    let name=path.file_name().unwrap_or_default().to_string_lossy();
    ["everything.exe","everything64.exe","everything32.exe"].iter().any(|known|name.eq_ignore_ascii_case(known))
}
fn same_path(a:&Path,b:&Path)->bool{crate::explorer_forwarder::same_path(a,b)}
fn class_name(instance:Option<&str>)->String{instance.map_or_else(||"EVERYTHING_TASKBAR_NOTIFICATION".into(),|name|format!("EVERYTHING_TASKBAR_NOTIFICATION_({name})"))}
fn find(instance:Option<&str>,expected:&Path)->Option<Connection>{
    let class=wide(&class_name(instance));let mut previous=None;
    for _ in 0..32{
        let window=unsafe{FindWindowExW(None,previous,PCWSTR(class.as_ptr()),None)}.ok()?;previous=Some(window);
        let Some(process)=identity(window)else{continue;};
        if !ordinary_everything(&process.executable)||!same_path(expected,&process.executable){continue;}
        return Some(Connection{hwnd:window,executable:process.executable,instance:instance.map(str::to_owned)});
    }None
}
fn external(configured:&Path)->Option<Connection>{
    if let Ok(instance)=std::env::var("ZSDESK_EVERYTHING_INSTANCE"){
        return find(Some(&instance),configured);
    }
    for instance in [None,Some("1.5a"),Some("1.5")]{if let Some(connection)=find(instance,configured){return Some(connection);}}
    // Automatically borrowing an arbitrary named instance can select a test
    // fixture or a different database. Only an explicitly chosen executable
    // permits discovering a user's custom named instance.
    let mut previous=None;
    while let Ok(window)=unsafe{FindWindowExW(None,previous,None,None)}{
        previous=Some(window);let mut buffer=[0u16;256];let length=unsafe{GetClassNameW(window,&mut buffer)};
        if length<=0{continue;}let class=String::from_utf16_lossy(&buffer[..length as usize]);
        let Some(instance)=class.strip_prefix("EVERYTHING_TASKBAR_NOTIFICATION_(").and_then(|name|name.strip_suffix(')'))else{continue;};
        if let Some(connection)=find(Some(instance),configured){return Some(connection);}
    }
    None
}
pub(super) fn available(configured:Option<&Path>)->bool{
    configured.is_some_and(|path|ensure_executable(path).is_ok())
}
pub(super) fn ensure_executable(path:&Path)->Result<()> {
    ensure!(path.is_absolute() && ordinary_everything(path) && path.is_file(),"请选择已安装的 Everything.exe 程序文件");
    let legacy = std::env::current_exe()?.parent().map(|dir|dir.join("engines/everything/Everything.exe"));
    ensure!(!legacy.is_some_and(|old|same_path(path,&old)),"请选择独立安装的 Everything.exe");
    Ok(())
}
fn lock(root:&Path,cancel:&impl Fn()->bool)->Result<StartLock>{
    let token=format!("{:x}",Sha256::digest(root.to_string_lossy().to_lowercase().as_bytes()));
    let name=wide(&format!("Local\\ZSdesk.Everything.Start.{token}"));let handle=Handle(unsafe{CreateMutexW(None,false,PCWSTR(name.as_ptr()))}?);
    let deadline=Instant::now()+Duration::from_secs(10);
    loop{if cancel(){bail!("搜索已取消");}
        let waited=unsafe{WaitForSingleObject(handle.0,50)};
        if waited==WAIT_OBJECT_0||waited==WAIT_ABANDONED{return Ok(StartLock(handle));}
        ensure!(waited==WAIT_TIMEOUT,"Everything 启动锁异常");ensure!(Instant::now()<deadline,"Everything 正在启动，请稍后重试");
    }
}
fn command(path:&Path)->Command{let mut command=Command::new(path);command.creation_flags(0x08000000).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());command}
pub(super) fn ensure_running(configured:Option<&Path>,cancel:&impl Fn()->bool)->Result<Connection> {
    let path=configured.context("请先在搜索设置中选择已安装的 Everything.exe")?;
    ensure_executable(path)?;
    if let Some(connection)=external(path){return Ok(connection);}
    let _lock=lock(path,cancel)?;
    if let Some(connection)=external(path){return Ok(connection);}
    if cancel(){bail!("搜索已取消");}
    let mut launch=command(path);
    if let Ok(instance)=std::env::var("ZSDESK_EVERYTHING_INSTANCE"){launch.arg("-instance").arg(instance);}
    launch.arg("-startup").spawn()?;
    let deadline=Instant::now()+Duration::from_secs(8);
    loop{
        if cancel(){bail!("搜索已取消");}
        if let Some(connection)=external(path){return Ok(connection);}
        ensure!(Instant::now()<deadline,"指定的 Everything 未就绪，请检查是否为支持 IPC 的版本");
        thread::sleep(Duration::from_millis(25));
    }
}
pub(super) fn ipc(window:HWND,code:usize,value:isize)->Result<usize>{
    let mut result=0;let sent=unsafe{SendMessageTimeoutW(window,WM_USER,WPARAM(code),LPARAM(value),SMTO_ABORTIFHUNG,1000,Some(&mut result))};
    ensure!(sent.0!=0,"Everything 暂时没有响应");Ok(result)
}
pub(super) fn wait_ready(connection:&Connection,cancel:&impl Fn()->bool,pending:&mut dyn FnMut())->Result<()>{
    if cancel(){bail!("搜索已取消");}
    wait_for_ready(||Ok(ipc(connection.hwnd,401,0)?!=0),cancel,pending,Duration::from_secs(120))
}
fn wait_for_ready(mut loaded:impl FnMut()->Result<bool>,cancel:&impl Fn()->bool,pending:&mut dyn FnMut(),budget:Duration)->Result<()>{
    let deadline=Instant::now()+budget;let mut notified=false;
    loop{
        if cancel(){bail!("搜索已取消");}
        if loaded()?{return Ok(());}
        if !notified{pending();notified=true;}
        ensure!(Instant::now()<deadline,"Everything 文件名索引仍在准备中，请稍后重试");
        thread::sleep(Duration::from_millis(25));
    }
}
pub(super) fn configured(profile:&Path)->Option<PathBuf>{crate::storage::load(profile).ok().and_then(|session|session.ui.everything_executable)}
pub(super) fn open_settings(profile:&Path)->Result<()>{
    let selected=configured(profile);let connection=ensure_running(selected.as_deref(),&||false)?;
    // Official SDK: EVERYTHING_IPC_ID_TRAY_OPTIONS, sent to the notification
    // window. This avoids creating another search window or changing config.
    unsafe{PostMessageW(Some(connection.hwnd),WM_COMMAND,WPARAM(40005),LPARAM(0))}?;Ok(())
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn selection_is_required_before_any_external_lookup_or_startup(){
        assert!(!available(None));
        assert!(ensure_running(None,&||panic!("no launch without a selection")).is_err());
        assert!(!available(Some(Path::new(r"missing\Everything.exe"))));
    }
    #[test]fn unread_database_never_becomes_a_successful_empty_snapshot(){
        use std::cell::Cell;
        let notifications=Cell::new(0);
        assert!(wait_for_ready(||Ok(false),&||false,&mut||notifications.set(notifications.get()+1),Duration::ZERO).is_err());
        assert_eq!(notifications.get(),1);
        assert!(wait_for_ready(||Err(anyhow::anyhow!("IPC unavailable")),&||false,&mut||panic!("IPC error is not indexing"),Duration::ZERO).is_err());
    }
    #[test]fn warm_database_has_no_wait_and_indexing_can_be_cancelled(){
        use std::cell::Cell;
        wait_for_ready(||Ok(true),&||false,&mut||panic!("ready database must not emit loading"),Duration::ZERO).unwrap();
        let cancelled=Cell::new(false);let probes=Cell::new(0);
        let error=wait_for_ready(||{probes.set(probes.get()+1);Ok(false)},&||cancelled.get(),&mut||cancelled.set(true),Duration::from_secs(1)).unwrap_err();
        assert_eq!(probes.get(),1);assert!(error.to_string().contains("取消"));
    }
}
