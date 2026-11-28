//! Installed application names and activation identities from the Windows Start menu.
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::HWND,
    System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED},
    UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem,
        KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY,
    },
};
use zsdesk_ui::menus::{self, MenuItemSpec};

struct Entry {
    name: String,
    searchable: String,
    pidl: Vec<u8>,
}
#[derive(Default)]
struct Catalog {
    entries: Arc<Vec<Entry>>,
    last: Option<Instant>,
    loading: bool,
    failed: bool,
}
fn catalog() -> &'static Mutex<Catalog> {
    static CACHE: OnceLock<Mutex<Catalog>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
fn enumerate() -> windows::core::Result<Vec<Entry>> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let folder: IShellItem =
        unsafe { SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) }?;
    let items: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems) }?;
    let mut result = Vec::new();
    loop {
        let mut batch = [None];
        unsafe { items.Next(&mut batch, None) }?;
        let Some(item) = batch[0].take() else {
            break;
        };
        let Ok(label) = (unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) }) else {
            continue;
        };
        let name = unsafe { label.to_string() };
        unsafe { CoTaskMemFree(Some(label.0.cast())) };
        let (Ok(name), Some(pidl)) = (name, zsdesk_windows::shell::pidl::item_pidl(&item)) else {
            continue;
        };
        if !name.trim().is_empty() {
            result.push(Entry {
                searchable: name.to_lowercase(),
                name,
                pidl,
            });
        }
    }
    result.sort_by(|a, b| {
        a.searchable
            .cmp(&b.searchable)
            .then_with(|| a.pidl.cmp(&b.pidl))
    });
    Ok(result)
}

pub(crate) fn warm() {
    let mut cache = catalog().lock().unwrap_or_else(|e| e.into_inner());
    let ttl = Duration::from_secs(if cache.failed { 5 } else { 60 });
    if cache.loading || cache.last.is_some_and(|at| at.elapsed() < ttl) {
        return;
    }
    cache.loading = true;
    if std::thread::Builder::new()
        .name("zsdesk-app-catalog".into())
        .spawn(|| {
            let entries = enumerate();
            let mut cache = catalog().lock().unwrap_or_else(|e| e.into_inner());
            cache.failed = entries.is_err();
            if let Ok(entries) = entries {
                cache.entries = Arc::new(entries);
            }
            cache.last = Some(Instant::now());
            cache.loading = false;
        })
        .is_err()
    {
        cache.loading = false;
        cache.failed = true;
        cache.last = Some(Instant::now());
    }
}

pub(crate) struct Matches {
    catalog: Arc<Vec<Entry>>,
    indices: Vec<usize>,
    empty: &'static str,
}
pub(crate) fn matches(query: &str) -> Matches {
    warm();
    let cache = catalog().lock().unwrap_or_else(|e| e.into_inner());
    let needle = query.trim().to_lowercase();
    let words: Vec<_> = needle.split_whitespace().collect();
    let mut indices: Vec<_> = cache
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| words.iter().all(|w| entry.searchable.contains(*w)))
        .map(|(i, _)| i)
        .collect();
    indices.sort_by_key(|i| {
        let name = &cache.entries[*i].searchable;
        (name != &needle, !name.starts_with(&needle))
    });
    let empty = if cache.failed {
        "无法读取应用列表"
    } else if cache.last.is_none() {
        "正在读取应用列表…"
    } else {
        "没有匹配的应用"
    };
    Matches {
        catalog: cache.entries.clone(),
        indices,
        empty,
    }
}
impl Matches {
    pub(crate) fn len(&self) -> usize {
        self.indices.len()
    }
    pub(crate) fn caption(&self) -> String {
        if self.indices.is_empty() {
            zsdesk_core::i18n::text("应用").to_owned()
        } else {
            format!(
                "{} ({})",
                zsdesk_core::i18n::text("应用"),
                self.indices.len()
            )
        }
    }
    pub(crate) fn rows(&self) -> Vec<MenuItemSpec> {
        if self.indices.is_empty() {
            return vec![menus::entry(
                zsdesk_core::i18n::text(self.empty),
                "empty",
                false,
                false,
            )];
        }
        self.indices
            .iter()
            .enumerate()
            .map(|(row, i)| menus::item(&self.catalog[*i].name, &format!("app-{row}")))
            .collect()
    }
    pub(crate) fn open(&self, owner: HWND, id: &str) -> windows::core::Result<()> {
        if let Some(index) = id
            .strip_prefix("app-")
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|row| self.indices.get(row))
        {
            zsdesk_windows::desktop::open(owner, &self.catalog[*index].pidl, None)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Reads the installed Windows AppsFolder without activating any application"]
    fn query_installed_apps_without_launching() {
        warm();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let state = catalog().lock().unwrap();
            if !state.loading {
                assert!(!state.failed);
                break;
            }
            drop(state);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        let all = matches("");
        assert!(!all.indices.is_empty());
        let name = &all.catalog[all.indices[0]].name;
        let exact = matches(&name.to_uppercase());
        let first = &exact.catalog[exact.indices[0]];
        assert_eq!(first.searchable, name.to_lowercase());
        assert!(zsdesk_windows::shell::pidl::valid(&first.pidl));
        assert_eq!(exact.rows().len(), exact.indices.len());
    }
}
