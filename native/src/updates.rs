use super::*;

// Stable two- or three-part release versions only. Unknown/special-edition/prerelease
// tags never become an update suggestion through an accidental lexical compare.
fn stable_version(tag:&str)->Option<[u32;3]> {
    let tag=tag.strip_prefix('v').or_else(||tag.strip_prefix('V')).unwrap_or(tag);
    let parts:Vec<_>=tag.split('.').collect();
    if parts.len()<2 || parts.len()>3 {return None;}
    let mut result=[0;3];
    for (i,part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|c|c.is_ascii_digit()) {return None;}
        result[i]=part.parse().ok()?;
    }
    Some(result)
}
fn release_comparison(remote:&str,current:&str)->String {
    match (stable_version(remote),stable_version(current)) {
        (Some(r),Some(c)) if r>c=>"正式版版本号高于当前版本。".into(),
        (Some(r),Some(c)) if r==c=>"版本号相同，没有更高版本。".into(),
        (Some(_),Some(_))=>"该正式版比当前版本更旧，没有更高版本。".into(),
        _=>"无法确认版本先后，不将它判定为新版本。".into(),
    }
}
impl App {
    pub(super) unsafe fn open_github(&self, releases: bool) {
        let url = if releases { "https://github.com/DLmember/Minecraft-Desktop-Pet/releases" } else { "https://github.com/DLmember/Minecraft-Desktop-Pet" };
        ShellExecuteW(self.panel, wide("open").as_ptr(), wide(url).as_ptr(), null(), null(), SW_SHOWNORMAL);
    }
    pub(super) unsafe fn check_update(&mut self) {
        if self.update_check.is_some() { return; }
        let (tx,rx) = std::sync::mpsc::channel();
        self.update_check = Some(rx);
        self.update_button(false);
        std::thread::spawn(move || {
            let result = (|| {
                let bytes = online::get("api.github.com", "/repos/DLmember/Minecraft-Desktop-Pet/releases/latest", 256*1024)?;
                let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if v["draft"].as_bool()!=Some(false) || v["prerelease"].as_bool()!=Some(false) { return Err("No stable release".into()); }
                let tag=v["tag_name"].as_str().ok_or("Missing version")?;
                if tag.len()>64 {return Err("Invalid version".into());}
                Ok(tag.to_owned())
            })();
            let _ = tx.send(result);
        });
    }
    unsafe fn update_button(&self, enabled: bool) {
        if let Some((_,h))=self.controls.iter().find(|x|x.0==170) {
            EnableWindow(*h,enabled as i32);
            SetWindowTextW(*h,wide(if enabled {"检查更新"} else {"查询中…"}).as_ptr());
        }
    }
    pub(super) unsafe fn poll_update(&mut self) {
        let Some(rx)=self.update_check.as_ref() else {return;};
        let result=match rx.try_recv() {
            Ok(v)=>v,
            Err(std::sync::mpsc::TryRecvError::Empty)=>return,
            Err(_)=>Err("Disconnected".into()),
        };
        self.update_check=None;
        self.update_button(true);
        match result {
            Ok(tag)=>{
                let comparison=release_comparison(&tag,VERSION);
                let message=format!("最新正式版：{tag}\n当前版本：v{VERSION}。\n{comparison}\n\n要打开 GitHub 发布页吗？");
                if MessageBoxW(self.panel,wide(&message).as_ptr(),wide("检查更新").as_ptr(),MB_YESNO|MB_ICONINFORMATION)==IDYES { self.open_github(true); }
            }
            Err(_)=>{MessageBoxW(self.panel,wide("Github又宕机了QAQ").as_ptr(),wide("检查更新").as_ptr(),MB_OK|MB_ICONINFORMATION);}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_versions_never_offer_older_or_equal_releases() {
        for tag in ["v2.4.12","v2.4.9","2.3.99","2.4.13"] {
            assert!(release_comparison(tag,"2.4.13").contains("没有更高版本"));
        }
        assert_eq!(stable_version("v2.5"),stable_version("v2.5.0"));
        assert!(stable_version("v2.4.10")>stable_version("v2.4.9"));
        assert!(release_comparison("v2.4.14","2.4.13").contains("高于"));
        assert!(release_comparison("v2.5.0","2.4.99").contains("高于"));
    }
    #[test]
    fn unknown_and_special_edition_tags_are_not_updates() {
        for tag in ["v2.4.14-beta","v2.4.14-blockbench","latest","v2","v2.4.13.1","v2.4.-1","v2.4.4294967296"] {
            assert!(stable_version(tag).is_none());
            assert!(release_comparison(tag,"2.4.13").contains("不将它判定为新版本"));
        }
    }
}
