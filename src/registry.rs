use std::io;
use winreg::enums::*;
use winreg::RegKey;

pub const CLASSES: &str = r"Software\Classes";
const BLOCKED: &str = r"Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked";
const FILE_EXTS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

/// (注册表键名, 中文名, 英文名)
pub const SCOPES: &[(&str, &str, &str)] = &[
    (r"*", "所有文件", "All files"),
    (r"AllFilesystemObjects", "文件+文件夹", "Files & folders"),
    (r"Directory", "文件夹", "Folders"),
    (r"Directory\Background", "背景/桌面", "Background/Desktop"),
    (r"Folder", "文件夹(通用)", "Folders (generic)"),
    (r"Drive", "驱动器", "Drives"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    Hklm,
    Hkcu,
}

impl Hive {
    pub fn key(self) -> RegKey {
        match self {
            Hive::Hklm => RegKey::predef(HKEY_LOCAL_MACHINE),
            Hive::Hkcu => RegKey::predef(HKEY_CURRENT_USER),
        }
    }
    pub fn label(self, en: bool) -> &'static str {
        match (self, en) {
            (Hive::Hklm, false) => "系统(HKLM)",
            (Hive::Hklm, true) => "System (HKLM)",
            (Hive::Hkcu, false) => "用户(HKCU)",
            (Hive::Hkcu, true) => "User (HKCU)",
        }
    }
    pub fn reg_prefix(self) -> &'static str {
        match self {
            Hive::Hklm => "HKLM",
            Hive::Hkcu => "HKCU",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// shell\<verb> 传统菜单命令
    Verb,
    /// shellex\ContextMenuHandlers 下的 COM 扩展
    ShellEx,
}

#[derive(Clone)]
pub struct MenuItem {
    pub kind: ItemKind,
    pub hive: Hive,
    pub scope_index: usize,
    pub key_name: String,
    pub display: String,
    /// Verb: 命令行；ShellEx: 处理程序 DLL 路径
    pub detail: String,
    pub clsid: Option<String>,
    pub enabled: bool,
    /// 仅按住 Shift 才显示
    pub extended: bool,
}

impl MenuItem {
    pub fn reg_path_rel(&self) -> String {
        let scope = SCOPES[self.scope_index].0;
        match self.kind {
            ItemKind::Verb => format!(r"{}\{}\shell\{}", CLASSES, scope, self.key_name),
            ItemKind::ShellEx => format!(
                r"{}\{}\shellex\ContextMenuHandlers\{}",
                CLASSES, scope, self.key_name
            ),
        }
    }
    pub fn reg_path_full(&self) -> String {
        format!(r"{}\{}", self.hive.reg_prefix(), self.reg_path_rel())
    }
}

pub fn is_admin() -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(CLASSES, KEY_WRITE)
        .is_ok()
}

pub fn scan() -> Vec<MenuItem> {
    let mut out = Vec::new();
    for hive in [Hive::Hklm, Hive::Hkcu] {
        for (si, (scope, _, _)) in SCOPES.iter().enumerate() {
            let shell_path = format!(r"{}\{}\shell", CLASSES, scope);
            if let Ok(shell) = hive.key().open_subkey(&shell_path) {
                for name in shell.enum_keys().flatten() {
                    if let Ok(k) = shell.open_subkey(&name) {
                        let mui = k
                            .get_value::<String, _>("MUIVerb")
                            .ok()
                            .filter(|s| !s.is_empty());
                        let def = k.get_value::<String, _>("").ok().filter(|s| !s.is_empty());
                        let display = mui.or(def).unwrap_or_else(|| name.clone());
                        let command = k
                            .open_subkey("command")
                            .and_then(|c| c.get_value::<String, _>(""))
                            .unwrap_or_default();
                        let enabled = k.get_raw_value("LegacyDisable").is_err();
                        let extended = k.get_raw_value("Extended").is_ok();
                        out.push(MenuItem {
                            kind: ItemKind::Verb,
                            hive,
                            scope_index: si,
                            key_name: name.clone(),
                            display,
                            detail: command,
                            clsid: None,
                            enabled,
                            extended,
                        });
                    }
                }
            }

            let ex_path = format!(r"{}\{}\shellex\ContextMenuHandlers", CLASSES, scope);
            if let Ok(exk) = hive.key().open_subkey(&ex_path) {
                for name in exk.enum_keys().flatten() {
                    let clsid = exk
                        .open_subkey(&name)
                        .ok()
                        .and_then(|k| k.get_value::<String, _>("").ok())
                        .map(|s| s.trim().to_string())
                        .filter(|s| s.starts_with('{'))
                        .unwrap_or_else(|| name.clone());
                    let (desc, dll) = clsid_info(&clsid);
                    let display = if desc.is_empty() { name.clone() } else { desc };
                    let enabled = !is_blocked(&clsid);
                    out.push(MenuItem {
                        kind: ItemKind::ShellEx,
                        hive,
                        scope_index: si,
                        key_name: name.clone(),
                        display,
                        detail: dll,
                        clsid: Some(clsid),
                        enabled,
                        extended: false,
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| {
        a.scope_index
            .cmp(&b.scope_index)
            .then_with(|| (a.kind == ItemKind::ShellEx).cmp(&(b.kind == ItemKind::ShellEx)))
            .then_with(|| a.display.to_lowercase().cmp(&b.display.to_lowercase()))
    });
    out
}

fn clsid_info(clsid: &str) -> (String, String) {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let base = format!(r"CLSID\{}", clsid);
    let desc = hkcr
        .open_subkey(&base)
        .and_then(|k| k.get_value::<String, _>(""))
        .unwrap_or_default();
    let dll = hkcr
        .open_subkey(format!(r"{}\InprocServer32", base))
        .and_then(|k| k.get_value::<String, _>(""))
        .unwrap_or_default();
    (desc, dll)
}

fn is_blocked(clsid: &str) -> bool {
    for hive in [Hive::Hklm, Hive::Hkcu] {
        if let Ok(k) = hive.key().open_subkey(BLOCKED) {
            if k.get_raw_value(clsid).is_ok() {
                return true;
            }
        }
    }
    false
}

pub fn set_verb_enabled(item: &MenuItem, enable: bool) -> io::Result<()> {
    let k = item
        .hive
        .key()
        .open_subkey_with_flags(item.reg_path_rel(), KEY_SET_VALUE)?;
    if enable {
        match k.delete_value("LegacyDisable") {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    } else {
        k.set_value("LegacyDisable", &"")
    }
}

pub fn set_shellex_enabled(clsid: &str, enable: bool) -> io::Result<()> {
    if clsid.is_empty() || !clsid.starts_with('{') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no recognizable CLSID; cannot disable via Blocked list",
        ));
    }
    if enable {
        for hive in [Hive::Hklm, Hive::Hkcu] {
            if let Ok(k) = hive.key().open_subkey_with_flags(BLOCKED, KEY_SET_VALUE) {
                let _ = k.delete_value(clsid);
            }
        }
        if is_blocked(clsid) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot remove from HKLM Blocked list (administrator required)",
            ));
        }
        Ok(())
    } else {
        let (k, _) = Hive::Hkcu.key().create_subkey(BLOCKED)?;
        k.set_value(clsid, &"Blocked by ctxmenu")
    }
}

pub fn delete_item(item: &MenuItem) -> io::Result<()> {
    item.hive.key().delete_subkey_all(item.reg_path_rel())
}

// ===================== “打开方式”列表管理 =====================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OwKind {
    /// HKCU FileExts\<ext>\OpenWithList：字母值 a,b,c... + MRUList
    MruList,
    /// OpenWithProgids：值名 = ProgID
    ProgidValues,
    /// OpenWithList：子键名 = exe
    ExeSubkeys,
    /// Applications\<exe>\SupportedTypes：应用自己注册支持的扩展名
    SupportedTypes,
    /// RegisteredApplications → Capabilities\FileAssociations（如 ima、浏览器）
    Capabilities,
}

impl OwKind {
    pub fn label(self, en: bool) -> &'static str {
        match self {
            OwKind::MruList => {
                if en {
                    "Recent (MRU)"
                } else {
                    "最近使用(MRU)"
                }
            }
            OwKind::ProgidValues => "OpenWithProgids",
            OwKind::ExeSubkeys => "OpenWithList",
            OwKind::SupportedTypes => {
                if en {
                    "App registration"
                } else {
                    "应用注册"
                }
            }
            OwKind::Capabilities => "Capabilities",
        }
    }
    fn order(self) -> u8 {
        match self {
            OwKind::MruList => 0,
            OwKind::ProgidValues => 1,
            OwKind::ExeSubkeys => 2,
            OwKind::SupportedTypes => 3,
            OwKind::Capabilities => 4,
        }
    }
}

#[derive(Clone)]
pub struct OpenWithItem {
    pub hive: Hive,
    pub kind: OwKind,
    /// 归属扩展名（如 .txt）
    pub ext: String,
    /// 父键相对路径（值/子键所在的键）
    pub parent: String,
    /// 值名或子键名（MRU 为字母；SupportedTypes 为扩展名）
    pub entry: String,
    /// exe 名或 ProgID
    pub app: String,
    pub display: String,
    pub detail: String,
    /// Capabilities 条目：RegisteredApplications 里的注册应用名（用于按应用归组）
    pub owner: Option<String>,
    /// 应用键上已有 NoOpenWith 标记（已从“打开方式”隐藏）
    pub hidden: bool,
}

impl OpenWithItem {
    pub fn parent_full(&self) -> String {
        format!(r"{}\{}", self.hive.reg_prefix(), self.parent)
    }
}

pub fn normalize_ext(input: &str) -> String {
    format!(".{}", input.trim().trim_start_matches('.').to_lowercase())
}

/// 把输入拆成多个扩展名（支持空格/逗号/分号分隔），去重、保持顺序
pub fn parse_exts(input: &str) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut v = Vec::new();
    for part in input.split([' ', ',', ';', '，', '、']) {
        if part.trim().is_empty() {
            continue;
        }
        let e = normalize_ext(part);
        if e.len() > 1 && seen.insert(e.clone()) {
            v.push(e);
        }
    }
    v
}

fn app_reg_sub(app: &str) -> String {
    if app.to_lowercase().ends_with(".exe") {
        format!(r"Applications\{}", app)
    } else {
        app.to_string()
    }
}

fn app_hidden(app: &str) -> bool {
    RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(app_reg_sub(app))
        .map(|k| k.get_raw_value("NoOpenWith").is_ok())
        .unwrap_or(false)
}

fn resolve_exe(exe: &str) -> (String, String) {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let base = format!(r"Applications\{}", exe);
    let friendly = hkcr
        .open_subkey(&base)
        .ok()
        .and_then(|k| k.get_value::<String, _>("FriendlyAppName").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| exe.to_string());
    let cmd = hkcr
        .open_subkey(format!(r"{}\shell\open\command", base))
        .and_then(|k| k.get_value::<String, _>(""))
        .unwrap_or_default();
    (friendly, cmd)
}

fn resolve_progid(progid: &str) -> (String, String) {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let friendly = hkcr
        .open_subkey(progid)
        .ok()
        .and_then(|k| {
            k.get_value::<String, _>("FriendlyTypeName")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| k.get_value::<String, _>("").ok().filter(|s| !s.is_empty()))
        })
        .unwrap_or_else(|| progid.to_string());
    let cmd = hkcr
        .open_subkey(format!(r"{}\shell\open\command", progid))
        .and_then(|k| k.get_value::<String, _>(""))
        .unwrap_or_default();
    (friendly, cmd)
}

fn push_progid_values(out: &mut Vec<OpenWithItem>, hive: Hive, ext: &str, path: &str) {
    if let Ok(k) = hive.key().open_subkey(path) {
        for (name, _v) in k.enum_values().flatten() {
            if name.is_empty() {
                continue;
            }
            let (display, detail) = resolve_progid(&name);
            out.push(OpenWithItem {
                hive,
                kind: OwKind::ProgidValues,
                ext: ext.to_string(),
                parent: path.to_string(),
                entry: name.clone(),
                app: name.clone(),
                display,
                detail,
                owner: None,
                hidden: app_hidden(&name),
            });
        }
    }
}

/// 扫描一个或多个扩展名“打开方式”列表的所有来源
pub fn scan_openwith(input: &str) -> Vec<OpenWithItem> {
    let mut out = Vec::new();
    for ext in parse_exts(input) {
        scan_one_ext(&ext, &mut out);
    }
    out.sort_by(|a, b| {
        a.ext
            .cmp(&b.ext)
            .then_with(|| a.display.to_lowercase().cmp(&b.display.to_lowercase()))
            .then_with(|| a.kind.order().cmp(&b.kind.order()))
    });
    out
}

fn scan_one_ext(ext: &str, out: &mut Vec<OpenWithItem>) {
    // 1) HKCU Explorer FileExts（MRU + Progids）
    let fe_list = format!(r"{}\{}\OpenWithList", FILE_EXTS, ext);
    if let Ok(k) = Hive::Hkcu.key().open_subkey(&fe_list) {
        for (name, _v) in k.enum_values().flatten() {
            if name.eq_ignore_ascii_case("MRUList") {
                continue;
            }
            let exe: String = k.get_value(&name).unwrap_or_default();
            if exe.is_empty() {
                continue;
            }
            let (display, detail) = resolve_exe(&exe);
            out.push(OpenWithItem {
                hive: Hive::Hkcu,
                kind: OwKind::MruList,
                ext: ext.to_string(),
                parent: fe_list.clone(),
                entry: name,
                hidden: app_hidden(&exe),
                app: exe,
                display,
                detail,
                owner: None,
            });
        }
    }
    push_progid_values(
        out,
        Hive::Hkcu,
        ext,
        &format!(r"{}\{}\OpenWithProgids", FILE_EXTS, ext),
    );

    // 2) Software\Classes\<ext> 和 SystemFileAssociations（含感知类型，如 text）
    let mut assoc_bases = vec![
        format!(r"{}\{}", CLASSES, ext),
        format!(r"{}\SystemFileAssociations\{}", CLASSES, ext),
    ];
    if let Ok(k) = RegKey::predef(HKEY_CLASSES_ROOT).open_subkey(&ext) {
        if let Ok(pt) = k.get_value::<String, _>("PerceivedType") {
            if !pt.is_empty() {
                assoc_bases.push(format!(r"{}\SystemFileAssociations\{}", CLASSES, pt));
            }
        }
    }
    for hive in [Hive::Hklm, Hive::Hkcu] {
        for base in &assoc_bases {
            let listp = format!(r"{}\OpenWithList", base);
            if let Ok(k) = hive.key().open_subkey(&listp) {
                for sub in k.enum_keys().flatten() {
                    let (display, detail) = resolve_exe(&sub);
                    out.push(OpenWithItem {
                        hive,
                        kind: OwKind::ExeSubkeys,
                        ext: ext.to_string(),
                        parent: listp.clone(),
                        entry: sub.clone(),
                        hidden: app_hidden(&sub),
                        app: sub,
                        display,
                        detail,
                        owner: None,
                    });
                }
            }
            push_progid_values(out, hive, ext, &format!(r"{}\OpenWithProgids", base));
        }
    }

    // 3) Applications\*\SupportedTypes 里注册了该扩展名的应用
    for hive in [Hive::Hklm, Hive::Hkcu] {
        let apps_path = format!(r"{}\Applications", CLASSES);
        if let Ok(apps) = hive.key().open_subkey(&apps_path) {
            for exe in apps.enum_keys().flatten() {
                let st_path = format!(r"{}\{}\SupportedTypes", apps_path, exe);
                if let Ok(st) = hive.key().open_subkey(&st_path) {
                    if st.get_raw_value(ext).is_ok() {
                        let (display, detail) = resolve_exe(&exe);
                        out.push(OpenWithItem {
                            hive,
                            kind: OwKind::SupportedTypes,
                            ext: ext.to_string(),
                            parent: st_path,
                            entry: ext.to_string(),
                            hidden: app_hidden(&exe),
                            app: exe,
                            display,
                            detail,
                            owner: None,
                        });
                    }
                }
            }
        }
    }

    // 5) RegisteredApplications → Capabilities\FileAssociations
    //    （ima、浏览器等通过“默认程序”机制注册的打开方式）
    for hive in [Hive::Hklm, Hive::Hkcu] {
        if let Ok(ra) = hive.key().open_subkey(r"Software\RegisteredApplications") {
            for (app_name, _v) in ra.enum_values().flatten() {
                if app_name.is_empty() {
                    continue;
                }
                let cap_path: String = match ra.get_value(&app_name) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let fa_path = format!(r"{}\FileAssociations", cap_path);
                let progid: String = match hive
                    .key()
                    .open_subkey(&fa_path)
                    .and_then(|k| k.get_value(ext))
                {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let (display, detail) = resolve_progid(&progid);
                out.push(OpenWithItem {
                    hive,
                    kind: OwKind::Capabilities,
                    ext: ext.to_string(),
                    parent: fa_path,
                    entry: ext.to_string(),
                    hidden: app_hidden(&progid),
                    app: progid,
                    display,
                    detail,
                    owner: Some(app_name),
                });
            }
        }
    }
}

/// 移除单条“打开方式”来源条目
pub fn remove_openwith(item: &OpenWithItem) -> io::Result<()> {
    match item.kind {
        OwKind::MruList => {
            let k = item
                .hive
                .key()
                .open_subkey_with_flags(&item.parent, KEY_QUERY_VALUE | KEY_SET_VALUE)?;
            k.delete_value(&item.entry)?;
            if let Ok(mru) = k.get_value::<String, _>("MRUList") {
                let letter = item.entry.chars().next().unwrap_or('\0');
                let newmru: String = mru.chars().filter(|c| *c != letter).collect();
                k.set_value("MRUList", &newmru)?;
            }
            Ok(())
        }
        OwKind::ProgidValues | OwKind::SupportedTypes | OwKind::Capabilities => {
            let k = item
                .hive
                .key()
                .open_subkey_with_flags(&item.parent, KEY_SET_VALUE)?;
            k.delete_value(&item.entry)
        }
        OwKind::ExeSubkeys => item
            .hive
            .key()
            .delete_subkey_all(format!(r"{}\{}", item.parent, item.entry)),
    }
}

/// 找出同一应用注册的整族 ProgID（如 Antigravity.txt / Antigravity.md / ...）。
/// 按第一个 . 之前的前缀在 HKLM/HKCU 的 Software\Classes 下枚举匹配项。
fn progid_family(progid: &str) -> Vec<String> {
    if !progid.contains('.') {
        return vec![progid.to_string()];
    }
    let prefix = format!("{}.", progid.split('.').next().unwrap().to_lowercase());
    let mut fam = std::collections::BTreeSet::new();
    fam.insert(progid.to_string());
    for hive in [Hive::Hklm, Hive::Hkcu] {
        if let Ok(k) = hive.key().open_subkey(CLASSES) {
            for name in k.enum_keys().flatten() {
                if name.len() > prefix.len() && name.to_lowercase().starts_with(&prefix) {
                    fam.insert(name);
                }
            }
        }
    }
    fam.into_iter().collect()
}

fn hide_targets(app: &str) -> Vec<String> {
    if app.to_lowercase().ends_with(".exe") {
        vec![format!(r"Applications\{}", app)]
    } else {
        progid_family(app)
    }
}

/// 写 NoOpenWith 标记隐藏应用（写 HKCU 覆盖层，可逆）。
/// exe 应用标记 Applications\<exe> 一处即全局生效；
/// ProgID 应用（如 Antigravity.txt）会把同前缀整族 ProgID 一起标记，
/// 从所有扩展名的“打开方式”里消失。返回 (标记键数, 说明)。
pub fn hide_app(app: &str) -> io::Result<(usize, String)> {
    let targets = hide_targets(app);
    let mut n = 0usize;
    for t in &targets {
        let sub = format!(r"{}\{}", CLASSES, t);
        let (k, _) = Hive::Hkcu.key().create_subkey(&sub)?;
        k.set_value("NoOpenWith", &"")?;
        n += 1;
    }
    let desc = if targets.len() == 1 {
        format!(r"HKCU\{}\{}", CLASSES, targets[0])
    } else {
        format!("{} 等 {} 个 ProgID", targets[0], targets.len())
    };
    Ok((n, desc))
}

/// 移除 NoOpenWith 标记（整族、两个 hive 都清）
pub fn unhide_app(app: &str) -> io::Result<()> {
    for t in &hide_targets(app) {
        let sub = format!(r"{}\{}", CLASSES, t);
        for hive in [Hive::Hklm, Hive::Hkcu] {
            if let Ok(k) = hive.key().open_subkey_with_flags(&sub, KEY_SET_VALUE) {
                let _ = k.delete_value("NoOpenWith");
            }
        }
    }
    if app_hidden(app) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "some NoOpenWith marks could not be removed (administrator required)",
        ));
    }
    Ok(())
}

/// 新建自定义菜单项，写入 HKCU（无需管理员，仅当前用户生效）。
/// 返回创建的键名。
pub fn add_custom(
    scope_index: usize,
    display: &str,
    command: &str,
    icon: &str,
    extended: bool,
) -> io::Result<String> {
    let scope = SCOPES[scope_index].0;
    let base: String = display.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let key_name = if base.is_empty() {
        format!("ctxmenu.{}", crate::backup::timestamp())
    } else {
        format!("ctxmenu.{}", base)
    };
    let path = format!(r"{}\{}\shell\{}", CLASSES, scope, key_name);
    let (k, _) = Hive::Hkcu.key().create_subkey(&path)?;
    k.set_value("", &display)?;
    if !icon.is_empty() {
        k.set_value("Icon", &icon)?;
    }
    if extended {
        k.set_value("Extended", &"")?;
    }
    let (c, _) = k.create_subkey("command")?;
    c.set_value("", &command)?;
    Ok(key_name)
}
