//! Read-only release check against the project's fixed HTTPS API endpoint.
use anyhow::{Result,ensure};
use windows::{core::w,Win32::Networking::WinHttp::*};
struct Internet(*mut core::ffi::c_void);impl Drop for Internet{fn drop(&mut self){let _=unsafe{WinHttpCloseHandle(self.0)};}}
fn owned(handle:*mut core::ffi::c_void)->Result<Internet>{if handle.is_null(){Err(windows::core::Error::from_thread().into())}else{Ok(Internet(handle))}}
fn version(tag:&str)->Option<[u64;3]>{let values=tag.trim_start_matches('v').split('.').map(str::parse::<u64>).collect::<std::result::Result<Vec<_>,_>>().ok()?;values.try_into().ok()}
pub(crate) fn check()->Result<String>{
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);
    let session=owned(unsafe{WinHttpOpen(w!("ZSDesk release check"),WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,None,None,0)})?;
    unsafe{WinHttpSetTimeouts(session.0,5000,5000,5000,5000)?;}
    let connection=owned(unsafe{WinHttpConnect(session.0,w!("api.github.com"),443,0)})?;
    let request=owned(unsafe{WinHttpOpenRequest(connection.0,w!("GET"),w!("/repos/qiu7824/zsdesk/releases/latest"),None,None,std::ptr::null(),WINHTTP_FLAG_SECURE)})?;
    unsafe{WinHttpSendRequest(request.0,None,None,0,0,0)?;WinHttpReceiveResponse(request.0,std::ptr::null_mut())?;}
    let mut status=0u32;let mut size=4;
    unsafe{WinHttpQueryHeaders(request.0,WINHTTP_QUERY_STATUS_CODE|WINHTTP_QUERY_FLAG_NUMBER,None,Some((&mut status as*mut u32).cast()),&mut size,std::ptr::null_mut())?;}
    ensure!(status==200,"更新服务返回 HTTP {status}");let mut data=Vec::new();
    loop{ensure!(std::time::Instant::now()<deadline,"更新检查超时");let mut buffer=[0u8;4096];let mut count=0;unsafe{WinHttpReadData(request.0,buffer.as_mut_ptr().cast(),buffer.len()as u32,&mut count)?;}if count==0{break;}ensure!(data.len()+count as usize<=1024*1024,"更新响应超出限制");data.extend_from_slice(&buffer[..count as usize]);}
    let value:serde_json::Value=serde_json::from_slice(&data)?;
    ensure!(value["draft"]==false&&value["prerelease"]==false,"服务未返回正式发布版本");
    let tag=value["tag_name"].as_str().ok_or_else(||anyhow::anyhow!("发布版本号缺失"))?;
    let remote=version(tag).ok_or_else(||anyhow::anyhow!("发布版本号格式无效"))?;
    let current=version(env!("CARGO_PKG_VERSION")).unwrap_or([0,0,0]);
    Ok(if remote>current{format!("发现新版本 {tag} · 可在项目发布页下载")}
        else if remote==current{"当前已是最新正式版本".into()}else{format!("当前本地版本较新；最新正式版为 {tag}")})
}
#[cfg(test)]mod tests{use super::*;
    #[test]fn release_versions_compare_numerically_and_reject_non_release_values(){assert!(version("v0.3.12")>version("v0.3.9"));assert!(version("0.4.0")>version("0.3.99"));assert!(version("1.0.0-beta").is_none());assert!(version("https://invalid").is_none());}
    #[test]#[ignore="Read-only live official release endpoint check"]
    fn native_official_release_check_reports_version()->Result<()>{let message=check()?;println!("{message}");ensure!(!message.is_empty(),"release response empty");Ok(())}
}
