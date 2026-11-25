use std::{ffi::OsString, fs, io, os::windows::fs::MetadataExt, path::Path};
use std::collections::HashMap;
use windows::Win32::UI::Shell::*;
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL};

#[derive(Clone, Copy)]
pub(super) struct SystemIcon { pub images: isize, pub index: i32 }

#[derive(Default)]
pub(super) struct TypeIcons(HashMap<(bool, OsString), SystemIcon>);

impl TypeIcons {
    pub fn get(&mut self, path: &Path, folder: bool, individual:bool) -> Option<SystemIcon> {
        use std::os::windows::ffi::OsStrExt;
        let ext = path.extension().unwrap_or_default().to_ascii_lowercase();
        let per_item = path.parent().is_none() || (individual && !folder && ["exe","ico","lnk","url"].iter().any(|e| ext.eq_ignore_ascii_case(e)));
        let key = (folder, if folder { OsString::new() } else { ext });
        if !per_item {
            if let Some(icon) = self.0.get(&key) { return Some(*icon); }
        }
        let mut info = SHFILEINFOW::default();
        let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut flags = SHGFI_SYSICONINDEX | SHGFI_SMALLICON;
        if !per_item { flags |= SHGFI_USEFILEATTRIBUTES; }
        let handle = unsafe { SHGetFileInfoW(windows::core::PCWSTR(name.as_ptr()),
            if folder { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL },
            Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, flags) };
        if handle == 0 { return None; }
        let icon = SystemIcon { images: handle as isize, index: info.iIcon };
        if !per_item && self.0.len() < 256 { self.0.insert(key, icon); }
        Some(icon)
    }
}
pub(super) struct Entry {
    pub name: OsString,
    pub folder: bool,
    pub icon: Option<SystemIcon>,
}
#[derive(Clone, Copy)]
pub(super) struct Visibility {
    hidden: bool,
    protected: bool,
}
impl Visibility {
    pub fn current() -> Self {
        use windows::{
            core::w,
            Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
        };
        let mut enabled = 0u32;
        let mut size = 4u32;
        let protected = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"),
                w!("ShowSuperHidden"),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut enabled as *mut u32).cast()),
                Some(&mut size),
            )
        }
        .is_ok()
            && enabled != 0;
        Self {
            hidden: zsdesk_windows::shell::commands::folder_settings().0,
            protected,
        }
    }
    fn includes(self, attributes: u32) -> bool {
        attributes & 2 == 0 || (self.hidden && (attributes & 4 == 0 || self.protected))
    }
}
/// Both panes apply the same visibility policy. A preview limits retained rows but counts every visible child.
pub(super) fn entries(
    path: &Path,
    visibility: Visibility,
    limit: usize,
    cancel: impl Fn() -> bool,
) -> io::Result<(Vec<Entry>, usize)> {
    let mut rows = Vec::new();
    let mut icons = TypeIcons::default();
    let mut count = 0;
    for (index, entry) in fs::read_dir(path)?.enumerate() {
        if index % 128 == 0 && cancel() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "目录读取已取消"));
        }
        let entry = entry?;
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(e) if matches!(e.kind(),io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied) => continue,
            Err(e) => return Err(e),
        };
        let attrs = metadata.file_attributes();
        if !visibility.includes(attrs) {
            continue;
        }
        count += 1;
        if rows.len() < limit {
            rows.push(Entry {
                name: entry.file_name(),
                icon: icons.get(&entry.path(), attrs & 16 != 0, limit <= 500),
                folder: attrs & 16 != 0,
            });
        }
    }
    rows.sort_by_cached_key(|entry| (!entry.folder, entry.name.to_string_lossy().to_lowercase()));
    Ok((rows, count))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parent_and_preview_follow_visibility_and_count_the_full_folder() -> io::Result<()> {
        let dir = std::env::temp_dir().join(format!("zsdesk-column-list-{}", std::process::id()));
        fs::create_dir(&dir)?;
        struct Guard(std::path::PathBuf);
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _guard = Guard(dir.clone());
        fs::create_dir(dir.join("data"))?;
        fs::write(dir.join("readme.txt"), b"test")?;
        let visible = Visibility {
            hidden: true,
            protected: false,
        };
        let (all, count) = entries(&dir, visible, usize::MAX, || false)?;
        assert_eq!(count, 2);
        assert!(all[0].folder);
        assert_eq!(all[0].name, "data");
        let (one, count) = entries(&dir, visible, 1, || false)?;
        assert_eq!((one.len(), count), (1, 2));
        assert!(visible.includes(2) && !visible.includes(6));
        assert!(!Visibility {
            hidden: false,
            protected: false
        }
        .includes(2));
        assert!(entries(&dir, visible, usize::MAX, || true).is_err());
        Ok(())
    }
}
