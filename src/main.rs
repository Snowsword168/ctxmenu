#![windows_subsystem = "windows"]

mod backup;
mod registry;

use std::collections::{BTreeMap, BTreeSet};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use registry::{ItemKind, MenuItem, OpenWithItem, SCOPES};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Zh,
    En,
}

/// 双语文案：所有界面字符串就地给出中英两个版本
fn t(l: Lang, zh: &'static str, en: &'static str) -> &'static str {
    match l {
        Lang::Zh => zh,
        Lang::En => en,
    }
}

fn scope_label(l: Lang, i: usize) -> &'static str {
    match l {
        Lang::Zh => SCOPES[i].1,
        Lang::En => SCOPES[i].2,
    }
}

fn config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("ctxmenu.ini")))
        .unwrap_or_else(|| PathBuf::from("ctxmenu.ini"))
}

fn load_lang() -> Lang {
    match std::fs::read_to_string(config_path()) {
        Ok(s) if s.contains("lang=en") => Lang::En,
        _ => Lang::Zh,
    }
}

fn save_lang(l: Lang) {
    let _ = std::fs::write(
        config_path(),
        if l == Lang::En { "lang=en" } else { "lang=zh" },
    );
}

fn app_title(l: Lang) -> &'static str {
    t(l, "右键菜单管理器", "Context Menu Manager")
}

fn main() -> eframe::Result<()> {
    let lang = load_lang();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1150.0, 720.0])
            .with_min_inner_size([820.0, 480.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        app_title(lang),
        options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(App::new(lang)))
        }),
    )
}

/// egui 默认字体不含中文，从系统字体目录加载微软雅黑（或备选字体）
fn setup_fonts(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("cjk".to_owned(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("cjk".to_owned());
            ctx.set_fonts(fonts);
            return;
        }
    }
}

fn restart_explorer() {
    let _ = std::process::Command::new("taskkill")
        .args(["/f", "/im", "explorer.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    let _ = std::process::Command::new("explorer.exe").spawn();
}

fn open_backup_dir() {
    let dir = backup::backup_root();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("explorer").arg(&dir).spawn();
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Menu,
    OpenWith,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OwSort {
    Ext,
    App,
    Entry,
}

/// “打开方式”页签上收集的用户操作，本帧末尾统一执行
enum OwAction {
    Remove(usize),
    Hide(usize),
    Unhide(usize),
    GroupHide(Vec<String>, String),
    GroupUnhide(Vec<String>, String),
    SortBy(OwSort),
}

/// 应用聚合：同前缀的 exe / ProgID 归为一组
struct OwGroup {
    name: String,
    apps: Vec<String>,
    exts: BTreeSet<String>,
    rows: usize,
    hidden_all: bool,
    hidden_any: bool,
}

/// 归组键：exe 去掉 .exe 后缀；ProgID 取第一个 . 之前的前缀。
/// 这样 Antigravity.exe 与 Antigravity.txt/.md 会归入同一组。
fn group_key(app: &str) -> (String, String) {
    let base = if app.to_lowercase().ends_with(".exe") {
        &app[..app.len() - 4]
    } else {
        app.split('.').next().unwrap_or(app)
    };
    (base.to_lowercase(), base.to_string())
}

fn ow_groups(items: &[OpenWithItem]) -> Vec<OwGroup> {
    let mut map: BTreeMap<String, OwGroup> = BTreeMap::new();
    for it in items {
        // Capabilities 条目按注册应用名归组（如 ima.copilot.xxx → ima），
        // 其余按 exe / ProgID 前缀归组
        let basis = it.owner.as_deref().unwrap_or(&it.app);
        let (key, name) = group_key(basis);
        let g = map.entry(key).or_insert_with(|| OwGroup {
            name,
            apps: Vec::new(),
            exts: BTreeSet::new(),
            rows: 0,
            hidden_all: true,
            hidden_any: false,
        });
        if !g.apps.iter().any(|a| a.eq_ignore_ascii_case(&it.app)) {
            g.apps.push(it.app.clone());
        }
        g.exts.insert(it.ext.clone());
        g.rows += 1;
        g.hidden_all &= it.hidden;
        g.hidden_any |= it.hidden;
    }
    map.into_values().collect()
}

struct AddDialog {
    scope_index: usize,
    display: String,
    command: String,
    icon: String,
    extended: bool,
}

impl Default for AddDialog {
    fn default() -> Self {
        Self {
            scope_index: 0,
            display: String::new(),
            command: String::new(),
            icon: String::new(),
            extended: false,
        }
    }
}

struct App {
    lang: Lang,
    tab: Tab,
    items: Vec<MenuItem>,
    search: String,
    scope_filter: Option<usize>,
    kind_filter: Option<ItemKind>,
    status: String,
    is_admin: bool,
    confirm_delete: Option<MenuItem>,
    add_dialog: Option<AddDialog>,
    ow_ext: String,
    ow_items: Vec<OpenWithItem>,
    ow_sort: OwSort,
    ow_group_view: bool,
}

impl App {
    fn new(lang: Lang) -> Self {
        let mut app = Self {
            lang,
            tab: Tab::Menu,
            items: Vec::new(),
            search: String::new(),
            scope_filter: None,
            kind_filter: None,
            status: String::new(),
            is_admin: registry::is_admin(),
            confirm_delete: None,
            add_dialog: None,
            ow_ext: "txt md log ini".to_owned(),
            ow_items: Vec::new(),
            ow_sort: OwSort::Ext,
            ow_group_view: false,
        };
        app.refresh();
        app.rescan_ow();
        app.status = format!(
            "{}: {}",
            t(lang, "已扫描右键菜单项", "Context menu items scanned"),
            app.items.len()
        );
        app
    }

    fn refresh(&mut self) {
        self.items = registry::scan();
    }

    fn rescan_ow(&mut self) {
        self.ow_items = registry::scan_openwith(&self.ow_ext);
        self.sort_ow_items();
    }

    fn sort_ow_items(&mut self) {
        match self.ow_sort {
            OwSort::Ext => self.ow_items.sort_by(|a, b| {
                a.ext
                    .cmp(&b.ext)
                    .then_with(|| a.display.to_lowercase().cmp(&b.display.to_lowercase()))
                    .then_with(|| a.app.to_lowercase().cmp(&b.app.to_lowercase()))
            }),
            OwSort::App => self.ow_items.sort_by(|a, b| {
                a.display
                    .to_lowercase()
                    .cmp(&b.display.to_lowercase())
                    .then_with(|| a.app.to_lowercase().cmp(&b.app.to_lowercase()))
                    .then_with(|| a.ext.cmp(&b.ext))
            }),
            OwSort::Entry => self.ow_items.sort_by(|a, b| {
                a.app
                    .to_lowercase()
                    .cmp(&b.app.to_lowercase())
                    .then_with(|| a.ext.cmp(&b.ext))
            }),
        }
    }

    fn apply_toggle(&mut self, idx: usize) {
        let l = self.lang;
        let item = self.items[idx].clone();
        let target = !item.enabled;
        let res = match item.kind {
            ItemKind::Verb => registry::set_verb_enabled(&item, target),
            ItemKind::ShellEx => {
                registry::set_shellex_enabled(item.clsid.as_deref().unwrap_or(""), target)
            }
        };
        match res {
            Ok(()) => {
                let verb = if target {
                    t(l, "已启用", "Enabled")
                } else {
                    t(l, "已禁用", "Disabled")
                };
                self.status = format!("{}: {}", verb, item.display);
                self.refresh();
            }
            Err(e) => {
                self.status = format!(
                    "{}: {e} ({})",
                    t(l, "操作失败", "Operation failed"),
                    item.reg_path_full()
                );
            }
        }
    }

    fn do_delete(&mut self, item: &MenuItem) {
        let l = self.lang;
        match backup::backup_item(item) {
            Ok(path) => match registry::delete_item(item) {
                Ok(()) => {
                    self.status = format!(
                        "{}: {} ({}: {})",
                        t(l, "已删除", "Deleted"),
                        item.reg_path_full(),
                        t(l, "备份", "backup"),
                        path.display()
                    );
                }
                Err(e) => {
                    self.status = format!(
                        "{}: {e} ({})",
                        t(l, "删除失败", "Delete failed"),
                        item.reg_path_full()
                    );
                }
            },
            Err(e) => {
                self.status = format!(
                    "{}: {e}",
                    t(l, "备份失败，已取消删除", "Backup failed; delete cancelled")
                );
            }
        }
        self.refresh();
    }

    fn apply_ow_remove(&mut self, idx: usize) {
        let l = self.lang;
        let item = self.ow_items[idx].clone();
        match backup::backup_key_named(&item.parent_full(), &format!("openwith_{}", item.app)) {
            Ok(path) => match registry::remove_openwith(&item) {
                Ok(()) => {
                    self.status = format!(
                        "{}: {} ({}: {})",
                        t(l, "已移除", "Removed"),
                        item.display,
                        t(l, "备份", "backup"),
                        path.display()
                    );
                }
                Err(e) => {
                    self.status = format!(
                        "{}: {e} ({})",
                        t(l, "移除失败", "Remove failed"),
                        item.parent_full()
                    );
                }
            },
            Err(e) => {
                self.status = format!(
                    "{}: {e}",
                    t(l, "备份失败，已取消移除", "Backup failed; remove cancelled")
                );
            }
        }
        self.rescan_ow();
    }

    fn apply_ow_hide(&mut self, idx: usize) {
        let l = self.lang;
        let item = self.ow_items[idx].clone();
        match registry::hide_app(&item.app) {
            Ok((n, _desc)) => {
                self.status = format!(
                    "{}: {} ({} {})",
                    t(l, "已屏蔽", "Blocked"),
                    item.display,
                    n,
                    t(l, "个键已标记，可随时取消", "keys marked; reversible")
                );
            }
            Err(e) => self.status = format!("{}: {e}", t(l, "屏蔽失败", "Block failed")),
        }
        self.rescan_ow();
    }

    fn apply_ow_unhide(&mut self, idx: usize) {
        let l = self.lang;
        let item = self.ow_items[idx].clone();
        match registry::unhide_app(&item.app) {
            Ok(()) => {
                self.status = format!("{}: {}", t(l, "已取消屏蔽", "Unblocked"), item.display);
            }
            Err(e) => self.status = format!("{}: {e}", t(l, "取消屏蔽失败", "Unblock failed")),
        }
        self.rescan_ow();
    }

    fn apply_group_hide(&mut self, apps: &[String], name: &str) {
        let l = self.lang;
        let mut total = 0usize;
        let mut err: Option<String> = None;
        for app in apps {
            match registry::hide_app(app) {
                Ok((n, _)) => total += n,
                Err(e) => err = Some(e.to_string()),
            }
        }
        self.status = match err {
            None => format!(
                "{}: {} ({} {})",
                t(l, "已屏蔽", "Blocked"),
                name,
                total,
                t(l, "个键已标记，可随时取消", "keys marked; reversible")
            ),
            Some(e) => format!("{}: {name}: {e}", t(l, "屏蔽部分失败", "Block partly failed")),
        };
        self.rescan_ow();
    }

    fn apply_group_unhide(&mut self, apps: &[String], name: &str) {
        let l = self.lang;
        let mut err: Option<String> = None;
        for app in apps {
            if let Err(e) = registry::unhide_app(app) {
                err = Some(e.to_string());
            }
        }
        self.status = match err {
            None => format!("{}: {}", t(l, "已取消屏蔽", "Unblocked"), name),
            Some(e) => format!(
                "{}: {name}: {e}",
                t(l, "取消屏蔽部分失败", "Unblock partly failed")
            ),
        };
        self.rescan_ow();
    }

    fn ui_menu_tab(
        &mut self,
        ui: &mut egui::Ui,
        pending_toggle: &mut Option<usize>,
        pending_ask_delete: &mut Option<usize>,
    ) {
        let l = self.lang;
        let needle = self.search.to_lowercase();
        let visible: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                (self.scope_filter.is_none() || self.scope_filter == Some(it.scope_index))
                    && (self.kind_filter.is_none() || self.kind_filter == Some(it.kind))
                    && (needle.is_empty()
                        || it.display.to_lowercase().contains(&needle)
                        || it.key_name.to_lowercase().contains(&needle)
                        || it.detail.to_lowercase().contains(&needle))
            })
            .map(|(i, _)| i)
            .collect();

        ui.label(format!(
            "{} {} / {} {}",
            visible.len(),
            t(l, "项显示", "shown"),
            self.items.len(),
            t(
                l,
                "项（勾选 = 启用，取消勾选 = 禁用，均可随时恢复）",
                "total (checked = enabled; all changes reversible)"
            )
        ));
        ui.add_space(4.0);

        let items = &self.items;
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(50.0))
            .column(Column::initial(240.0).resizable(true).clip(true))
            .column(Column::exact(110.0))
            .column(Column::exact(95.0))
            .column(Column::exact(105.0))
            .column(Column::remainder().clip(true))
            .column(Column::exact(60.0))
            .header(24.0, |mut header| {
                header.col(|ui| {
                    ui.strong(t(l, "启用", "On"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "名称", "Name"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "位置", "Location"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "类型", "Type"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "来源", "Hive"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "命令 / 处理程序", "Command / Handler"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "操作", "Action"));
                });
            })
            .body(|mut body| {
                for &i in &visible {
                    let it = &items[i];
                    body.row(24.0, |mut row| {
                        row.col(|ui| {
                            let mut en = it.enabled;
                            if ui.checkbox(&mut en, "").changed() {
                                *pending_toggle = Some(i);
                            }
                        });
                        row.col(|ui| {
                            let mut text = it.display.clone();
                            if it.extended {
                                text.push_str("  [Shift]");
                            }
                            ui.label(text).on_hover_text(format!(
                                "{}: {}\n{}",
                                t(l, "键名", "Key"),
                                it.key_name,
                                it.reg_path_full()
                            ));
                        });
                        row.col(|ui| {
                            ui.label(scope_label(l, it.scope_index));
                        });
                        row.col(|ui| {
                            ui.label(match it.kind {
                                ItemKind::Verb => t(l, "菜单命令", "Menu command"),
                                ItemKind::ShellEx => t(l, "Shell扩展", "Shell ext"),
                            });
                        });
                        row.col(|ui| {
                            ui.label(it.hive.label(l == Lang::En));
                        });
                        row.col(|ui| {
                            let d = if it.detail.is_empty() {
                                it.clsid.clone().unwrap_or_default()
                            } else {
                                it.detail.clone()
                            };
                            ui.label(d.as_str()).on_hover_text(d.as_str());
                        });
                        row.col(|ui| {
                            if ui.button(t(l, "删除", "Delete")).clicked() {
                                *pending_ask_delete = Some(i);
                            }
                        });
                    });
                }
            });
    }

    fn ui_openwith_tab(&mut self, ui: &mut egui::Ui, actions: &mut Vec<OwAction>) {
        let l = self.lang;
        ui.horizontal(|ui| {
            ui.label(t(l, "扩展名(可多个):", "Extensions:"));
            let re = ui.add(egui::TextEdit::singleline(&mut self.ow_ext).desired_width(260.0));
            let enter = re.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button(t(l, "扫描", "Scan")).clicked() || enter {
                self.rescan_ow();
                self.status = format!(
                    "{} {} → {} {}",
                    registry::parse_exts(&self.ow_ext).len(),
                    t(l, "个扩展名", "extensions"),
                    self.ow_items.len(),
                    t(l, "条", "entries")
                );
            }
            if ui
                .button(t(l, "常用文本类型", "Common text types"))
                .on_hover_text(t(
                    l,
                    "填入 txt md log ini cfg conf json xml yaml yml csv 并扫描",
                    "Fill in txt md log ini cfg conf json xml yaml yml csv and scan",
                ))
                .clicked()
            {
                self.ow_ext = "txt md log ini cfg conf json xml yaml yml csv".to_owned();
                self.rescan_ow();
                self.status = format!(
                    "{}: {} {}",
                    t(l, "已扫描常用文本类型", "Common text types scanned"),
                    self.ow_items.len(),
                    t(l, "条", "entries")
                );
            }
            ui.checkbox(&mut self.ow_group_view, t(l, "按应用聚合", "Group by app"));
        });
        ui.add_space(4.0);

        if self.ow_group_view {
            ui.label(t(
                l,
                "每个应用一行。“屏蔽”对该应用的全部注册（整族 ProgID + Applications 键）写 NoOpenWith 标记，让它不再出现在任何文件类型的“打开方式”里；可随时取消，不删注册表。",
                "One row per application. \"Block\" marks all of its registrations (ProgID family + Applications key) with NoOpenWith so it disappears from \"Open with\" for every file type; reversible, nothing is deleted.",
            ));
            ui.add_space(4.0);
            self.ui_ow_group_table(ui, actions);
        } else {
            ui.label(t(
                l,
                "点击“扩展名 / 应用 / 条目”表头切换排序。“移除”只删该条注册来源（先自动备份）；“屏蔽应用”整族生效。改动对新打开的资源管理器窗口生效。",
                "Click the Ext / App / Entry headers to sort. \"Remove\" deletes just that registration (backed up first); \"Block app\" affects the whole family. Changes apply to newly opened Explorer windows.",
            ));
            ui.add_space(4.0);
            self.ui_ow_flat_table(ui, actions);
        }
    }

    fn ui_ow_flat_table(&self, ui: &mut egui::Ui, actions: &mut Vec<OwAction>) {
        let l = self.lang;
        if self.ow_items.is_empty() {
            ui.label(t(
                l,
                "（没有找到条目——输入扩展名后点“扫描”）",
                "(No entries — type extensions and click Scan)",
            ));
            return;
        }
        let sort = self.ow_sort;
        let items = &self.ow_items;
        let sort_header = |ui: &mut egui::Ui,
                           actions: &mut Vec<OwAction>,
                           this: OwSort,
                           label: &str| {
            let text = if sort == this {
                format!("{label} ▲")
            } else {
                label.to_owned()
            };
            if ui.selectable_label(sort == this, text).clicked() {
                actions.push(OwAction::SortBy(this));
            }
        };
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(70.0))
            .column(Column::initial(210.0).resizable(true).clip(true))
            .column(Column::exact(110.0))
            .column(Column::initial(170.0).resizable(true).clip(true))
            .column(Column::remainder().clip(true))
            .column(Column::exact(160.0))
            .header(24.0, |mut header| {
                header.col(|ui| {
                    sort_header(ui, actions, OwSort::Ext, t(l, "扩展名", "Ext"));
                });
                header.col(|ui| {
                    sort_header(ui, actions, OwSort::App, t(l, "应用", "App"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "存放位置", "Hive"));
                });
                header.col(|ui| {
                    sort_header(ui, actions, OwSort::Entry, t(l, "条目", "Entry"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "命令", "Command"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "操作", "Actions"));
                });
            })
            .body(|mut body| {
                for (i, it) in items.iter().enumerate() {
                    body.row(24.0, |mut row| {
                        row.col(|ui| {
                            ui.label(it.ext.as_str());
                        });
                        row.col(|ui| {
                            let mut text = it.display.clone();
                            if it.hidden {
                                text.push_str(t(l, "  [已屏蔽]", "  [blocked]"));
                            }
                            let mut hover = format!(
                                "{} | {}\n{}",
                                it.app,
                                it.kind.label(l == Lang::En),
                                it.parent_full()
                            );
                            if let Some(o) = &it.owner {
                                hover.push_str(&format!(
                                    "\n{}: {}",
                                    t(l, "注册应用", "Registered app"),
                                    o
                                ));
                            }
                            ui.label(text).on_hover_text(hover);
                        });
                        row.col(|ui| {
                            ui.label(it.hive.label(l == Lang::En))
                                .on_hover_text(it.parent_full());
                        });
                        row.col(|ui| {
                            ui.label(it.app.as_str()).on_hover_text(format!(
                                "{}: {}",
                                t(l, "值/子键名", "Value/subkey"),
                                it.entry
                            ));
                        });
                        row.col(|ui| {
                            ui.label(it.detail.as_str()).on_hover_text(it.detail.as_str());
                        });
                        row.col(|ui| {
                            if ui.button(t(l, "移除", "Remove")).clicked() {
                                actions.push(OwAction::Remove(i));
                            }
                            if it.hidden {
                                if ui.button(t(l, "取消屏蔽", "Unblock")).clicked() {
                                    actions.push(OwAction::Unhide(i));
                                }
                            } else if ui.button(t(l, "屏蔽应用", "Block app")).clicked() {
                                actions.push(OwAction::Hide(i));
                            }
                        });
                    });
                }
            });
    }

    fn ui_ow_group_table(&self, ui: &mut egui::Ui, actions: &mut Vec<OwAction>) {
        let l = self.lang;
        if self.ow_items.is_empty() {
            ui.label(t(
                l,
                "（没有找到条目——输入扩展名后点“扫描”）",
                "(No entries — type extensions and click Scan)",
            ));
            return;
        }
        let groups = ow_groups(&self.ow_items);
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(220.0).resizable(true).clip(true))
            .column(Column::initial(260.0).resizable(true).clip(true))
            .column(Column::exact(70.0))
            .column(Column::exact(110.0))
            .column(Column::exact(100.0))
            .header(24.0, |mut header| {
                header.col(|ui| {
                    ui.strong(t(l, "应用", "App"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "涉及扩展名", "Extensions"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "条目数", "Entries"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "状态", "Status"));
                });
                header.col(|ui| {
                    ui.strong(t(l, "操作", "Action"));
                });
            })
            .body(|mut body| {
                for g in &groups {
                    body.row(24.0, |mut row| {
                        row.col(|ui| {
                            ui.label(g.name.as_str()).on_hover_text(format!(
                                "{}:\n{}",
                                t(l, "注册标识", "Registered IDs"),
                                g.apps.join("\n")
                            ));
                        });
                        row.col(|ui| {
                            let s = g.exts.iter().cloned().collect::<Vec<_>>().join(" ");
                            ui.label(s.as_str()).on_hover_text(s.as_str());
                        });
                        row.col(|ui| {
                            ui.label(g.rows.to_string());
                        });
                        row.col(|ui| {
                            ui.label(if g.hidden_all {
                                t(l, "已屏蔽", "Blocked")
                            } else if g.hidden_any {
                                t(l, "部分屏蔽", "Partial")
                            } else {
                                ""
                            });
                        });
                        row.col(|ui| {
                            if g.hidden_all {
                                if ui.button(t(l, "取消屏蔽", "Unblock")).clicked() {
                                    actions.push(OwAction::GroupUnhide(
                                        g.apps.clone(),
                                        g.name.clone(),
                                    ));
                                }
                            } else if ui
                                .button(t(l, "屏蔽", "Block"))
                                .on_hover_text(t(
                                    l,
                                    "从所有文件类型的“打开方式”里隐藏该应用（可逆）",
                                    "Hide this app from \"Open with\" for all file types (reversible)",
                                ))
                                .clicked()
                            {
                                actions.push(OwAction::GroupHide(g.apps.clone(), g.name.clone()));
                            }
                        });
                    });
                }
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut pending_toggle: Option<usize> = None;
        let mut pending_ask_delete: Option<usize> = None;
        let mut ow_actions: Vec<OwAction> = Vec::new();
        let l = self.lang;

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Menu, t(l, "右键菜单", "Context Menu"));
                ui.selectable_value(
                    &mut self.tab,
                    Tab::OpenWith,
                    t(l, "“打开方式”列表", "\"Open With\" List"),
                );
                ui.separator();
                if ui
                    .button(t(l, "重启资源管理器", "Restart Explorer"))
                    .on_hover_text(t(
                        l,
                        "重启 explorer.exe 让改动立即生效",
                        "Restart explorer.exe to apply changes immediately",
                    ))
                    .clicked()
                {
                    restart_explorer();
                    self.status = t(l, "已重启资源管理器", "Explorer restarted").to_owned();
                }
                if ui.button(t(l, "打开备份目录", "Open backups")).clicked() {
                    open_backup_dir();
                }
                ui.separator();
                if ui.selectable_label(l == Lang::Zh, "中文").clicked() && l != Lang::Zh {
                    self.lang = Lang::Zh;
                    save_lang(Lang::Zh);
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Title(
                            app_title(Lang::Zh).to_owned(),
                        ));
                }
                if ui.selectable_label(l == Lang::En, "EN").clicked() && l != Lang::En {
                    self.lang = Lang::En;
                    save_lang(Lang::En);
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Title(
                            app_title(Lang::En).to_owned(),
                        ));
                }
                if !self.is_admin {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 150, 0),
                        t(
                            l,
                            "⚠ 未以管理员身份运行，系统级(HKLM)项目无法修改",
                            "⚠ Not elevated; HKLM entries cannot be modified",
                        ),
                    );
                }
            });
            ui.add_space(4.0);
            if self.tab == Tab::Menu {
                ui.horizontal(|ui| {
                    if ui.button(t(l, "刷新", "Refresh")).clicked() {
                        self.refresh();
                        self.status = format!(
                            "{}: {}",
                            t(l, "已刷新", "Refreshed"),
                            self.items.len()
                        );
                    }
                    if ui.button(t(l, "添加菜单项", "Add menu item")).clicked() {
                        self.add_dialog = Some(AddDialog::default());
                    }
                    if ui.button(t(l, "备份全部", "Backup all")).clicked() {
                        match backup::backup_all() {
                            Ok((dir, n)) => {
                                self.status = format!(
                                    "{}: {} → {}",
                                    t(l, "已导出 .reg 文件", "Exported .reg files"),
                                    n,
                                    dir.display()
                                );
                            }
                            Err(e) => {
                                self.status =
                                    format!("{}: {e}", t(l, "备份失败", "Backup failed"));
                            }
                        }
                    }
                    ui.separator();
                    ui.label(t(l, "搜索:", "Search:"));
                    ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(180.0));
                    ui.label(t(l, "位置:", "Location:"));
                    egui::ComboBox::from_id_salt("scope_filter")
                        .selected_text(
                            self.scope_filter
                                .map(|i| scope_label(l, i))
                                .unwrap_or(t(l, "全部位置", "All locations")),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.scope_filter,
                                None,
                                t(l, "全部位置", "All locations"),
                            );
                            for i in 0..SCOPES.len() {
                                ui.selectable_value(
                                    &mut self.scope_filter,
                                    Some(i),
                                    scope_label(l, i),
                                );
                            }
                        });
                    ui.label(t(l, "类型:", "Type:"));
                    egui::ComboBox::from_id_salt("kind_filter")
                        .selected_text(match self.kind_filter {
                            None => t(l, "全部类型", "All types"),
                            Some(ItemKind::Verb) => t(l, "菜单命令", "Menu command"),
                            Some(ItemKind::ShellEx) => t(l, "Shell扩展", "Shell ext"),
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.kind_filter,
                                None,
                                t(l, "全部类型", "All types"),
                            );
                            ui.selectable_value(
                                &mut self.kind_filter,
                                Some(ItemKind::Verb),
                                t(l, "菜单命令", "Menu command"),
                            );
                            ui.selectable_value(
                                &mut self.kind_filter,
                                Some(ItemKind::ShellEx),
                                t(l, "Shell扩展", "Shell ext"),
                            );
                        });
                });
            }
            ui.add_space(6.0);
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(3.0);
            ui.label(self.status.as_str());
            ui.add_space(3.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Menu => self.ui_menu_tab(ui, &mut pending_toggle, &mut pending_ask_delete),
            Tab::OpenWith => self.ui_openwith_tab(ui, &mut ow_actions),
        });

        // 添加菜单项对话框
        if let Some(dialog) = &mut self.add_dialog {
            let mut close = false;
            let mut submit = false;
            egui::Window::new(t(l, "添加自定义菜单项", "Add Custom Menu Item"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("add_grid")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(t(l, "显示名称:", "Display name:"));
                            ui.add(
                                egui::TextEdit::singleline(&mut dialog.display)
                                    .desired_width(360.0),
                            );
                            ui.end_row();

                            ui.label(t(l, "生效位置:", "Applies to:"));
                            egui::ComboBox::from_id_salt("add_scope")
                                .selected_text(scope_label(l, dialog.scope_index))
                                .show_ui(ui, |ui| {
                                    for i in 0..SCOPES.len() {
                                        ui.selectable_value(
                                            &mut dialog.scope_index,
                                            i,
                                            scope_label(l, i),
                                        );
                                    }
                                });
                            ui.end_row();

                            ui.label(t(l, "命令:", "Command:"));
                            ui.add(
                                egui::TextEdit::singleline(&mut dialog.command)
                                    .desired_width(360.0)
                                    .hint_text(t(
                                        l,
                                        r#"如 notepad.exe "%1"（背景/桌面用 %V）"#,
                                        r#"e.g. notepad.exe "%1" (use %V for background)"#,
                                    )),
                            );
                            ui.end_row();

                            ui.label(t(l, "图标(可选):", "Icon (optional):"));
                            ui.add(
                                egui::TextEdit::singleline(&mut dialog.icon)
                                    .desired_width(360.0)
                                    .hint_text(r"C:\Tools\app.exe,0"),
                            );
                            ui.end_row();

                            ui.label("");
                            ui.checkbox(
                                &mut dialog.extended,
                                t(l, "仅按住 Shift 时显示", "Show only while Shift is held"),
                            );
                            ui.end_row();
                        });
                    ui.small(t(
                        l,
                        "%1 = 选中的文件/文件夹路径，%V = 当前目录。新项写入 HKCU，仅当前用户生效。",
                        "%1 = selected file/folder path, %V = current directory. Written to HKCU (current user only).",
                    ));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let ok = !dialog.display.trim().is_empty()
                            && !dialog.command.trim().is_empty();
                        if ui
                            .add_enabled(ok, egui::Button::new(t(l, "创建", "Create")))
                            .clicked()
                        {
                            submit = true;
                        }
                        if ui.button(t(l, "取消", "Cancel")).clicked() {
                            close = true;
                        }
                    });
                });
            if submit {
                let d = self.add_dialog.take().unwrap();
                match registry::add_custom(
                    d.scope_index,
                    d.display.trim(),
                    d.command.trim(),
                    d.icon.trim(),
                    d.extended,
                ) {
                    Ok(key) => {
                        self.status = format!(
                            "{}: {} ({}: {})",
                            t(l, "已创建菜单项", "Menu item created"),
                            d.display.trim(),
                            t(l, "键名", "key"),
                            key
                        );
                        self.refresh();
                    }
                    Err(e) => {
                        self.status = format!("{}: {e}", t(l, "创建失败", "Create failed"));
                    }
                }
            } else if close {
                self.add_dialog = None;
            }
        }

        // 删除确认对话框
        if let Some(item) = self.confirm_delete.clone() {
            let mut close = false;
            let mut go = false;
            egui::Window::new(t(l, "确认删除", "Confirm Deletion"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("{}: {}", t(l, "名称", "Name"), item.display));
                    ui.monospace(item.reg_path_full());
                    ui.add_space(4.0);
                    ui.label(t(
                        l,
                        "删除前会自动导出 .reg 备份，双击备份文件即可恢复。",
                        "A .reg backup is exported before deleting; double-click it to restore.",
                    ));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(t(l, "备份并删除", "Backup & delete")).clicked() {
                            go = true;
                        }
                        if ui.button(t(l, "取消", "Cancel")).clicked() {
                            close = true;
                        }
                    });
                });
            if go {
                self.confirm_delete = None;
                self.do_delete(&item);
            } else if close {
                self.confirm_delete = None;
            }
        }

        if let Some(i) = pending_toggle {
            self.apply_toggle(i);
        }
        if let Some(i) = pending_ask_delete {
            self.confirm_delete = Some(self.items[i].clone());
        }
        if let Some(act) = ow_actions.into_iter().next() {
            match act {
                OwAction::Remove(i) => self.apply_ow_remove(i),
                OwAction::Hide(i) => self.apply_ow_hide(i),
                OwAction::Unhide(i) => self.apply_ow_unhide(i),
                OwAction::GroupHide(apps, name) => self.apply_group_hide(&apps, &name),
                OwAction::GroupUnhide(apps, name) => self.apply_group_unhide(&apps, &name),
                OwAction::SortBy(s) => {
                    self.ow_sort = s;
                    self.sort_ow_items();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_finds_items() {
        let items = crate::registry::scan();
        println!("scanned {} items", items.len());
        for it in items.iter().take(15) {
            println!(
                "[{}] {} | {} | {}",
                if it.enabled { "on " } else { "off" },
                it.reg_path_full(),
                it.display,
                it.detail
            );
        }
        assert!(!items.is_empty(), "本机不可能一个右键菜单项都没有");
    }

    #[test]
    fn openwith_multi_scan() {
        let items = crate::registry::scan_openwith("txt, md log .ini");
        println!("openwith multi: {} entries", items.len());
        for it in &items {
            println!(
                "[{}] {} | {} | {} | {}",
                if it.hidden { "hid" } else { "   " },
                it.ext,
                it.kind.label(false),
                it.app,
                it.display
            );
        }
        assert!(!items.is_empty(), ".txt 应该至少有一个打开方式来源");
        assert!(items.iter().any(|i| i.ext == ".md"), "应扫到 .md 条目");
    }

    #[test]
    fn group_key_merges_exe_and_progid() {
        let (k1, _) = super::group_key("Antigravity.exe");
        let (k2, _) = super::group_key("Antigravity.txt");
        let (k3, _) = super::group_key("Antigravity.md");
        assert_eq!(k1, k2);
        assert_eq!(k2, k3);
        let (k4, _) = super::group_key("txtfile");
        assert_eq!(k4, "txtfile");
    }
}
