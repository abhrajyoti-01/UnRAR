//! UnRAR — WinRAR-style archive manager (egui/eframe).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use egui::{Align, Color32, Key, Layout, RichText, TextureHandle};

use crate::engine::{self, ArchiveInfo, Entry, Format};
use crate::icons::Icons;
use crate::viewer::Viewer;

const APP_TITLE: &str = "UnRAR";

// ---------------------------------------------------------------- messages

enum Msg {
    Done(String),
    Err(String),
    ViewData { name: String, data: Vec<u8> },
}

// ---------------------------------------------------------------- helpers

fn fmt_size(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut f = n as f64;
    for (i, u) in U.iter().enumerate() {
        if f < 1024.0 || i == 4 {
            return if i == 0 { format!("{} {}", n, u) }
                   else { format!("{:.1} {}", f, u) };
        }
        f /= 1024.0;
    }
    String::new()
}

fn appdata() -> PathBuf {
    let d = std::env::var("APPDATA").map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    d.join("UnRAR")
}

fn load_recent() -> Vec<PathBuf> {
    std::fs::read_to_string(appdata().join("recent.txt"))
        .map(|s| s.lines().map(PathBuf::from).collect())
        .unwrap_or_default()
}

fn save_recent(list: &[PathBuf]) {
    let _ = std::fs::create_dir_all(appdata());
    let body = list.iter().map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>().join("\n");
    let _ = std::fs::write(appdata().join("recent.txt"), body);
}

fn fmt_mtime(dt: std::time::SystemTime) -> String {
    let secs = dt.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0) as i64;
    engine::epoch_time(secs).unwrap_or_default()
}

// ---------------------------------------------------------------- dialogs

#[derive(Default)]
struct AddDialog {
    open: bool,
    name: String,
    format_idx: usize,          // 0=zip, 1=tar, 2=tar.gz, 3=tar.bz2, 4=tar.xz
    level_idx: usize,           // index into engine::LEVELS
    password: String,
    comment: String,
    make_sfx: bool,
    solid: bool,
    recovery: bool,
    lock: bool,
    dict_idx: usize,
    volume: String,
    files: Vec<PathBuf>,
}

#[derive(Default)]
struct ExtractDialog {
    open: bool,
    dest: String,
    password: String,
    members: Vec<String>,       // empty = all
}

struct UnRARApp {
    icons: Icons,

    // browse mode (filesystem)
    cwd: PathBuf,
    fs_list: Vec<FsEntry>,

    // archive mode
    archive: Option<PathBuf>,
    info: Option<ArchiveInfo>,
    in_dir: String,             // prefix inside archive ("a/b/")

    selected: HashSet<String>,
    sort_key: SortKey,
    sort_desc: bool,
    status_left: String,
    status_right: String,
    find_open: bool,
    find_text: String,
    recent: Vec<PathBuf>,
    level_idx: usize,

    viewers: Vec<Viewer>,
    add_dlg: AddDialog,
    extract_dlg: ExtractDialog,
    show_info: bool,
    show_comment: bool,
    comment_text: String,
    show_about: bool,

    rx: Option<Receiver<Msg>>,
    busy: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum SortKey { Name, Size, Packed, Type, Modified, Crc, Ratio, Method }

struct FsEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
    modified: String,
}

// ---------------------------------------------------------------- app

impl UnRARApp {
    pub fn new(cc: &eframe::CreationContext, start: Option<PathBuf>) -> Self {
        let icons = Icons::load(&cc.egui_ctx);
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut app = UnRARApp {
            icons,
            cwd: cwd.clone(),
            fs_list: Vec::new(),
            archive: None,
            info: None,
            in_dir: String::new(),
            selected: HashSet::new(),
            sort_key: SortKey::Name,
            sort_desc: false,
            status_left: "Ready".into(),
            status_right: String::new(),
            find_open: false,
            find_text: String::new(),
            recent: load_recent(),
            level_idx: 3,
            viewers: Vec::new(),
            add_dlg: AddDialog::default(),
            extract_dlg: ExtractDialog::default(),
            show_info: false,
            show_comment: false,
            comment_text: String::new(),
            show_about: false,
            rx: None,
            busy: None,
        };
        app.refresh_fs();
        if let Some(p) = start {
            app.try_open(p);
        }
        app
    }

    // ---------------- mode helpers ----------------

    fn in_archive(&self) -> bool { self.archive.is_some() }

    fn refresh_fs(&mut self) {
        self.fs_list.clear();
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.cwd) {
            for e in rd.flatten() {
                let md = e.metadata().ok();
                let is_dir = md.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let ent = FsEntry {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path: e.path(),
                    is_dir,
                    size: md.as_ref().map(|m| m.len()).unwrap_or(0),
                    modified: md.and_then(|m| m.modified().ok())
                        .map(fmt_mtime).unwrap_or_default(),
                };
                if is_dir { dirs.push(ent); } else { files.push(ent); }
            }
        }
        dirs.sort_by_key(|e| e.name.to_lowercase());
        files.sort_by_key(|e| e.name.to_lowercase());
        dirs.extend(files);
        self.fs_list = dirs;
    }

    fn loc_text(&self) -> String {
        match &self.archive {
            Some(p) => {
                if self.in_dir.is_empty() {
                    p.to_string_lossy().into_owned()
                } else {
                    format!("{}\\{}", p.to_string_lossy(), self.in_dir
                        .trim_end_matches('/').replace('/', "\\"))
                }
            }
            None => self.cwd.to_string_lossy().into_owned(),
        }
    }

    fn set_status(&mut self, s: impl Into<String>) {
        self.status_left = s.into();
    }

    fn push_recent(&mut self, p: &Path) {
        self.recent.retain(|x| x != p);
        self.recent.insert(0, p.to_path_buf());
        self.recent.truncate(8);
        save_recent(&self.recent);
    }

    // ---------------- archive ops ----------------

    fn try_open(&mut self, path: PathBuf) {
        match engine::list_archive(&path) {
            Ok(info) => {
                self.archive = Some(path.clone());
                self.in_dir.clear();
                self.selected.clear();
                self.push_recent(&path);
                self.info = Some(info);
                self.set_status(format!("Opened {}", path.display()));
            }
            Err(e) => {
                self.set_status(format!("Cannot open {}: {}", path.display(), e));
            }
        }
    }

    fn close_archive(&mut self) {
        self.archive = None;
        self.info = None;
        self.in_dir.clear();
        self.selected.clear();
        self.refresh_fs();
        self.set_status("Archive closed");
    }

    fn run_bg(&mut self, label: &str,
              f: impl FnOnce() -> Result<Msg, String> + Send + 'static) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.busy = Some(label.to_string());
        std::thread::spawn(move || {
            let msg = match f() {
                Ok(m) => m,
                Err(e) => Msg::Err(e),
            };
            let _ = tx.send(msg);
        });
    }

    // display rows for archive mode: children of self.in_dir
    fn visible_rows(&self) -> Vec<Row> {
        let Some(info) = &self.info else { return Vec::new() };
        let mut dirs: Vec<Row> = Vec::new();
        let mut files: Vec<Row> = Vec::new();
        let mut seen_dirs: HashSet<String> = HashSet::new();
        let prefix = &self.in_dir;
        for e in &info.entries {
            if !e.name.starts_with(prefix.as_str()) { continue; }
            let rest = &e.name[prefix.len()..];
            if rest.is_empty() { continue; }
            if let Some(pos) = rest.find('/') {
                let d = format!("{}{}/", prefix, &rest[..pos]);
                if seen_dirs.insert(d.clone()) {
                    dirs.push(Row::folder(&rest[..pos]));
                }
            } else {
                files.push(Row::entry(e));
            }
        }
        // explicit dir entries (trailing slash dirs)
        let mut rows = dirs;
        rows.extend(files);
        // sorting
        let key = self.sort_key;
        let desc = self.sort_desc;
        rows.sort_by(|a, b| {
            let ord = match key {
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortKey::Size => a.size.cmp(&b.size),
                SortKey::Packed => a.packed.cmp(&b.packed),
                SortKey::Type => a.kind.cmp(&b.kind),
                SortKey::Modified => a.mtime.cmp(&b.mtime),
                SortKey::Crc => a.crc.cmp(&b.crc),
                SortKey::Ratio => a.ratio.cmp(&b.ratio),
                SortKey::Method => a.method.cmp(&b.method),
            };
            if desc { ord.reverse() } else { ord }
        });
        rows.sort_by_key(|r| !r.is_dir);   // folders first
        rows
    }

    fn go_up(&mut self) {
        if self.in_archive() {
            if !self.in_dir.is_empty() {
                let t = self.in_dir.trim_end_matches('/');
                match t.rfind('/') {
                    Some(i) => self.in_dir = t[..i + 1].to_string(),
                    None => self.in_dir.clear(),
                }
            } else {
                self.close_archive();
                return;
            }
        } else if let Some(p) = self.cwd.parent() {
            self.cwd = p.to_path_buf();
            self.refresh_fs();
        }
        self.selected.clear();
    }
}

// a display row: either a real entry or a synthesized folder
struct Row {
    name: String,        // display name (may be dir component)
    full: String,        // full member path or dir prefix
    size: u64, packed: u64,
    kind: String, mtime: String,
    crc: String, ratio: String, method: String,
    is_dir: bool, encrypted: bool,
}

impl Row {
    fn entry(e: &Entry) -> Row {
        let disp = e.name.rsplit('/').next().unwrap_or(&e.name).to_string();
        let ext = Path::new(&e.name).extension()
            .map(|x| x.to_string_lossy().to_uppercase() + " file")
            .unwrap_or_else(|| "file".into());
        Row {
            name: disp, full: e.name.clone(),
            size: e.size, packed: e.packed,
            kind: if e.is_dir { "Folder".into() } else { ext },
            mtime: e.mtime.clone().unwrap_or_default(),
            crc: e.crc.map(|c| format!("{:08X}", c)).unwrap_or_default(),
            ratio: e.ratio(), method: e.method.clone(),
            is_dir: e.is_dir, encrypted: e.encrypted,
        }
    }
    fn folder(name: &str) -> Row {
        Row {
            name: name.to_string(), full: name.to_string(),
            size: 0, packed: 0, kind: "Folder".into(),
            mtime: String::new(), crc: String::new(),
            ratio: String::new(), method: String::new(),
            is_dir: true, encrypted: false,
        }
    }
}

// ---------------------------------------------------------------- actions

impl UnRARApp {
    fn cmd_open_dialog(&mut self) {
        let dlg = rfd::FileDialog::new()
            .add_filter("Archives", &["rar", "rev", "r00", "zip", "zipx",
                "jar", "7z", "tar", "gz", "bz2", "xz", "zst", "cab", "tgz",
                "tbz2", "txz", "exe"])
            .add_filter("All files", &["*"]);
        if let Some(p) = dlg.pick_file() {
            self.try_open(p);
        }
    }

    fn cmd_add(&mut self) {
        if self.in_archive() {
            // add files to current archive
            let fmt = engine::detect_format(self.archive.as_ref().unwrap());
            if fmt != Format::Zip {
                self.set_status("Updating is supported for ZIP archives only");
                return;
            }
            if let Some(files) = rfd::FileDialog::new()
                .set_title("Files to add").pick_files() {
                let arc = self.archive.clone().unwrap();
                let lvl = engine::LEVELS[self.level_idx].1;
                self.run_bg("Adding…", move || {
                    engine::zip_add(&arc, &files, lvl, None, None)
                        .map(|n| Msg::Done(format!("Added {n} file(s)")))
                        .map_err(|e| e.to_string())
                });
            }
        } else {
            // pick files then open Add dialog to choose archive
            if let Some(files) = rfd::FileDialog::new()
                .set_title("Files to archive").pick_files() {
                self.add_dlg.files = files;
                self.add_dlg.name = self.cwd.join("archive.zip")
                    .to_string_lossy().into_owned();
                self.add_dlg.open = true;
            }
        }
    }

    fn cmd_extract(&mut self) {
        if !self.in_archive() { return; }
        let mut d = ExtractDialog::default();
        d.dest = self.cwd.to_string_lossy().into_owned();
        d.members = self.selected.iter().cloned().collect();
        if d.members.is_empty() && !self.in_dir.is_empty() {
            d.members = vec![self.in_dir.clone()];
        }
        self.extract_dlg = d;
        self.extract_dlg.open = true;
    }

    fn cmd_test(&mut self) {
        let Some(arc) = self.archive.clone() else { return };
        self.run_bg("Testing…", move || {
            engine::test_archive(&arc)
                .map(Msg::Done).map_err(|e| e.to_string())
        });
    }

    fn cmd_view(&mut self) {
        let Some(arc) = self.archive.clone() else {
            // browse mode: open selected file in viewer
            if let Some(name) = self.selected.iter().next().cloned() {
                let p = self.cwd.join(&name);
                if p.is_file() {
                    if let Ok(d) = std::fs::read(&p) {
                        self.viewers.push(Viewer::new(&name, d));
                    }
                }
            }
            return;
        };
        let Some(name) = self.selected.iter().next().cloned() else { return };
        let pwd_holder = self.selected_password(name.clone());
        self.run_bg("Reading…", move || {
            engine::read_member(&arc, &name, pwd_holder.as_deref())
                .map(|data| Msg::ViewData { name, data })
                .map_err(|e| e.to_string())
        });
    }

    fn selected_password(&self, _name: String) -> Option<String> {
        // if archive has encrypted entries, we'd prompt — handled by dialog
        None
    }

    fn cmd_delete(&mut self) {
        let Some(arc) = self.archive.clone() else { return };
        if engine::detect_format(&arc) != Format::Zip {
            self.set_status("Delete is supported for ZIP archives only");
            return;
        }
        if self.selected.is_empty() { return; }
        let names: Vec<String> = self.selected.iter().cloned().collect();
        let n = names.len();
        self.run_bg("Deleting…", move || {
            engine::zip_delete(&arc, &names)
                .map(|_| Msg::Done(format!("Deleted {n} entr(y/ies)")))
                .map_err(|e| e.to_string())
        });
    }

    fn cmd_lock(&mut self) {
        let Some(arc) = self.archive.clone() else { return };
        let Ok(md) = std::fs::metadata(&arc) else { return };
        let mut p = md.permissions();
        let ro = p.readonly();
        p.set_readonly(!ro);
        if std::fs::set_permissions(&arc, p).is_ok() {
            self.set_status(if ro { "Archive unlocked" }
                            else { "Archive locked (read-only)" });
        }
    }

    fn cmd_scan(&mut self) {
        let Some(arc) = self.archive.clone() else { return };
        self.run_bg("Scanning…", move || {
            let defender = r"C:\Program Files\Windows Defender\MpCmdRun.exe";
            let out = std::process::Command::new(defender)
                .args(["-Scan", "-ScanType", "3", "-File"])
                .arg(&arc)
                .output();
            match out {
                Ok(o) if o.status.success() =>
                    Ok(Msg::Done("Windows Defender: no threats found".into())),
                Ok(o) => Ok(Msg::Done(format!(
                    "Defender exited {} — see Windows Security", o.status))),
                Err(e) => Err(format!("Cannot run Defender: {e}")),
            }
        });
    }

    fn cmd_sfx(&mut self) {
        let Some(arc) = self.archive.clone() else { return };
        if engine::detect_format(&arc) != Format::Zip {
            self.set_status("SFX builds from ZIP archives only");
            return;
        }
        if let Some(out) = rfd::FileDialog::new()
            .set_title("Create self-extracting archive")
            .set_file_name("archive.exe")
            .add_filter("Executable", &["exe"]).save_file() {
            self.run_bg("Building SFX…", move || {
                engine::make_sfx(&arc, &out)
                    .map(|_| Msg::Done("SFX created".into()))
                    .map_err(|e| e.to_string())
            });
        }
    }

    fn cmd_comment(&mut self) {
        let Some(arc) = self.archive.clone() else { return };
        self.comment_text = match engine::detect_format(&arc) {
            Format::Zip => engine::zip_comment(&arc).unwrap_or_default(),
            _ => String::new(),
        };
        self.show_comment = true;
    }

    // ---------------- rendering ----------------

    fn toolbar_button(&self, ui: &mut egui::Ui, icon: &str, text: &str)
        -> egui::Response {
        let tex: TextureHandle = self.icons.get(icon);
        ui.add(egui::Button::image_and_text(
            egui::Image::new(egui::load::SizedTexture::from_handle(&tex))
                .fit_to_exact_size(egui::vec2(26.0, 26.0)),
            text))
    }

    fn menubar(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            if ui.button("Open archive…    Ctrl+O").clicked() {
                ui.close(); self.cmd_open_dialog();
            }
            if ui.button("Close archive\tCtrl+W").clicked() {
                ui.close(); self.close_archive();
            }
            ui.separator();
            ui.menu_button("Recent archives", |ui| {
                if self.recent.is_empty() {
                    ui.label("(empty)");
                }
                for p in self.recent.clone() {
                    if ui.button(p.to_string_lossy()).clicked() {
                        ui.close(); self.try_open(p);
                    }
                }
            });
            ui.separator();
            if ui.button("Exit").clicked() {
                std::process::exit(0);
            }
        });
        ui.menu_button("Commands", |ui| {
            if ui.button("Add files to archive…").clicked() {
                ui.close(); self.cmd_add();
            }
            if ui.button("Extract to…    Ctrl+E").clicked() {
                ui.close(); self.cmd_extract();
            }
            if ui.button("Test archive    Ctrl+T").clicked() {
                ui.close(); self.cmd_test();
            }
            if ui.button("View file    Enter").clicked() {
                ui.close(); self.cmd_view();
            }
            if ui.button("Delete    Del").clicked() {
                ui.close(); self.cmd_delete();
            }
            ui.separator();
            if ui.button("Find…    Ctrl+F").clicked() {
                ui.close(); self.find_open = !self.find_open;
            }
            if ui.button("Comment…").clicked() {
                ui.close(); self.cmd_comment();
            }
            if ui.button("Lock/Unlock").clicked() {
                ui.close(); self.cmd_lock();
            }
            ui.separator();
            if ui.button("Scan for viruses").clicked() {
                ui.close(); self.cmd_scan();
            }
            if ui.button("Create SFX…").clicked() {
                ui.close(); self.cmd_sfx();
            }
            ui.separator();
            if ui.button("Archive information    Ctrl+I").clicked() {
                ui.close(); self.show_info = true;
            }
        });
        ui.menu_button("Options", |ui| {
            ui.label("Compression level");
            for (i, (name, _)) in engine::LEVELS.iter().enumerate() {
                if ui.radio_value(&mut self.level_idx, i, *name).clicked() {
                    ui.close();
                }
            }
        });
        ui.menu_button("Help", |ui| {
            if ui.button("About UnRAR…").clicked() {
                ui.close(); self.show_about = true;
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        if self.toolbar_button(ui, "open", "Open").clicked()
            { self.cmd_open_dialog(); }
        if self.toolbar_button(ui, "new", "New").clicked()
            { self.cmd_new_archive(); }
        ui.separator();
        if self.toolbar_button(ui, "add", "Add").clicked()
            { self.cmd_add(); }
        if self.toolbar_button(ui, "extract", "Extract to").clicked()
            { self.cmd_extract(); }
        if self.toolbar_button(ui, "test", "Test").clicked()
            { self.cmd_test(); }
        if self.toolbar_button(ui, "view", "View").clicked()
            { self.cmd_view(); }
        if self.toolbar_button(ui, "delete", "Delete").clicked()
            { self.cmd_delete(); }
        ui.separator();
        if self.toolbar_button(ui, "find", "Find").clicked()
            { self.find_open = !self.find_open; }
        if self.toolbar_button(ui, "comment", "Comment").clicked()
            { self.cmd_comment(); }
        if self.toolbar_button(ui, "lock", "Lock").clicked()
            { self.cmd_lock(); }
        if self.toolbar_button(ui, "info", "Info").clicked()
            { self.show_info = true; }
        ui.separator();
        if self.toolbar_button(ui, "help", "About").clicked()
            { self.show_about = true; }
    }

    fn cmd_new_archive(&mut self) {
        self.add_dlg.files.clear();
        self.add_dlg.name = self.cwd.join("archive.zip")
            .to_string_lossy().into_owned();
        self.add_dlg.open = true;
    }

    fn location_bar(&mut self, ui: &mut egui::Ui) {
        let up: TextureHandle = self.icons.get("up");
        if ui.add(egui::Button::image(
                egui::load::SizedTexture::from_handle(&up))).clicked() {
            self.go_up();
        }
        let mut txt = self.loc_text();
        ui.add_enabled(false, egui::TextEdit::singleline(&mut txt)
            .desired_width(f32::INFINITY));
    }

    fn rows_archive(&mut self, ui: &mut egui::Ui) {
        let rows = self.visible_rows();
        let find = self.find_text.to_lowercase();
        let rows: Vec<Row> = if find.is_empty() { rows } else {
            rows.into_iter()
                .filter(|r| r.name.to_lowercase().contains(&find))
                .collect()
        };
        self.render_table(ui, rows);
    }

    fn rows_fs(&mut self, ui: &mut egui::Ui) {
        let find = self.find_text.to_lowercase();
        let rows: Vec<Row> = self.fs_list.iter()
            .filter(|e| find.is_empty()
                || e.name.to_lowercase().contains(&find))
            .map(|e| {
                let ext = e.path.extension()
                    .map(|x| x.to_string_lossy().to_uppercase() + " file")
                    .unwrap_or_else(|| "file".into());
                Row {
                    name: e.name.clone(), full: e.name.clone(),
                    size: e.size, packed: 0,
                    kind: if e.is_dir { "Folder".into() } else { ext },
                    mtime: e.modified.clone(),
                    crc: String::new(), ratio: String::new(),
                    method: String::new(),
                    is_dir: e.is_dir, encrypted: false,
                }
            }).collect();
        self.render_table(ui, rows);
    }

    fn render_table(&mut self, ui: &mut egui::Ui, rows: Vec<Row>) {
        use egui_extras::{Column, TableBuilder};
        let mut clicked_col: Option<SortKey> = None;
        let mut activate: Option<Row> = None;
        let mut toggled: Option<String> = None;
        let mut ctx_menu = false;
        let mut ctx_pos = egui::pos2(0.0, 0.0);

        let mut col_hdr = |ui: &mut egui::Ui, label: &str, key: SortKey| {
            let suffix = if self.sort_key == key {
                if self.sort_desc { " ▼" } else { " ▲" }
            } else { "" };
            let resp = ui.add(egui::Button::new(
                format!("{label}{suffix}")).frame(false)
                .min_size(egui::vec2(ui.available_width(), 0.0)));
            if resp.clicked() { clicked_col = Some(key); }
        };

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(280.0).resizable(true))   // name
            .column(Column::initial(80.0).resizable(true))    // size
            .column(Column::initial(80.0).resizable(true))    // packed
            .column(Column::initial(80.0).resizable(true))    // type
            .column(Column::initial(120.0).resizable(true))   // modified
            .column(Column::initial(80.0).resizable(true))    // crc
            .column(Column::initial(55.0).resizable(true))    // ratio
            .column(Column::remainder().resizable(true));     // method

        table = table.sense(egui::Sense::click());
        table.header(20.0, |mut h| {
            h.col(|ui| col_hdr(ui, "Name", SortKey::Name));
            h.col(|ui| col_hdr(ui, "Size", SortKey::Size));
            h.col(|ui| col_hdr(ui, "Packed", SortKey::Packed));
            h.col(|ui| col_hdr(ui, "Type", SortKey::Type));
            h.col(|ui| col_hdr(ui, "Modified", SortKey::Modified));
            h.col(|ui| col_hdr(ui, "CRC32", SortKey::Crc));
            h.col(|ui| col_hdr(ui, "Ratio", SortKey::Ratio));
            h.col(|ui| col_hdr(ui, "Method", SortKey::Method));
        }).body(|body| {
            body.rows(20.0, rows.len(), |mut row| {
                let r = &rows[row.index()];
                let sel = self.selected.contains(&r.full);
                row.set_selected(sel);
                // name cell with icon
                row.col(|ui| {
                    let icon = if r.is_dir { self.icons.get("folder") }
                               else { self.icons.get("file") };
                    ui.horizontal(|ui| {
                        ui.add(egui::Image::new(
                                egui::load::SizedTexture::from_handle(&icon))
                            .fit_to_exact_size(egui::vec2(16.0, 16.0)));
                        let lbl = if r.encrypted {
                            RichText::new(&r.name).color(Color32::LIGHT_RED)
                        } else { RichText::new(&r.name) };
                        ui.label(lbl);
                    });
                });
                row.col(|ui| { ui.label(if r.is_dir { "".into() }
                                        else { fmt_size(r.size) }); });
                row.col(|ui| { ui.label(if r.is_dir { "".into() }
                                        else { fmt_size(r.packed) }); });
                row.col(|ui| { ui.label(&r.kind); });
                row.col(|ui| { ui.label(&r.mtime); });
                row.col(|ui| { ui.label(&r.crc); });
                row.col(|ui| { ui.label(&r.ratio); });
                row.col(|ui| { ui.label(&r.method); });

                let resp = row.response();
                if resp.clicked() {
                    toggled = Some(r.full.clone());
                }
                if resp.double_clicked() {
                    activate = Some(Row {
                        name: r.name.clone(), full: r.full.clone(),
                        size: r.size, packed: r.packed,
                        kind: r.kind.clone(), mtime: r.mtime.clone(),
                        crc: r.crc.clone(), ratio: r.ratio.clone(),
                        method: r.method.clone(),
                        is_dir: r.is_dir, encrypted: r.encrypted,
                    });
                }
                if resp.secondary_clicked() {
                    toggled = Some(r.full.clone());
                    ctx_menu = true;
                    ctx_pos = resp.interact_pointer_pos()
                        .unwrap_or_else(|| egui::pos2(100.0, 100.0));
                }
            });
        });

        if let Some(k) = clicked_col {
            if self.sort_key == k { self.sort_desc = !self.sort_desc; }
            else { self.sort_key = k; self.sort_desc = false; }
        }
        if let Some(f) = toggled {
            let mods = ui.ctx().input(|i| i.modifiers);
            if mods.ctrl {
                if !self.selected.remove(&f) { self.selected.insert(f); }
            } else if mods.shift {
                self.selected.insert(f);
            } else {
                self.selected.clear();
                self.selected.insert(f);
            }
            self.update_stats();
        }
        if let Some(r) = activate {
            self.activate_row(&r);
        }
        if ctx_menu {
            egui::Window::new("ctx")
                .title_bar(false).resizable(false).collapsible(false)
                .fixed_pos(ctx_pos).auto_sized()
                .show(ui.ctx(), |ui| {
                    if ui.button("View").clicked() { self.cmd_view(); }
                    if ui.button("Extract to…").clicked() { self.cmd_extract(); }
                    if ui.button("Select all").clicked() {
                        for r in &self.visible_rows() {
                            self.selected.insert(r.full.clone());
                        }
                    }
                    if ui.button("Delete").clicked() { self.cmd_delete(); }
                });
        }
    }

    fn activate_row(&mut self, r: &Row) {
        if self.in_archive() {
            if r.is_dir {
                if r.full.ends_with('/') {
                    self.in_dir = r.full.clone();
                } else {
                    self.in_dir = format!("{}/", r.full);
                }
                self.selected.clear();
            } else {
                self.selected.clear();
                self.selected.insert(r.full.clone());
                self.cmd_view();
            }
        } else {
            let p = self.cwd.join(&r.full);
            if r.is_dir {
                self.cwd = p;
                self.refresh_fs();
                self.selected.clear();
            } else {
                // try open as archive, else view file
                match engine::detect_format(&p) {
                    Format::Unknown => {
                        if let Ok(d) = std::fs::read(&p) {
                            self.viewers.push(Viewer::new(&r.name, d));
                        }
                    }
                    _ => self.try_open(p),
                }
            }
        }
    }

    fn update_stats(&mut self) {
        if let Some(info) = &self.info {
            let total: u64 = info.entries.iter().map(|e| e.size).sum();
            let mut left = format!("{} objects — {}", info.entries.len(),
                                   fmt_size(total));
            if !self.selected.is_empty() {
                let sel: u64 = self.selected.iter()
                    .filter_map(|n| info.entries.iter().find(|e| e.name == *n))
                    .map(|e| e.size).sum();
                left += &format!("   |   {} selected — {}",
                                 self.selected.len(), fmt_size(sel));
            }
            self.status_left = left;
            let mut right = info.format.label().to_string();
            if info.encrypted_headers
                || info.entries.iter().any(|e| e.encrypted) {
                right += " | encrypted";
            }
            if info.multipart { right += " | multivolume"; }
            if info.sfx { right += " | SFX"; }
            self.status_right = right;
        } else {
            self.status_left = format!("{} items", self.fs_list.len());
            self.status_right = self.cwd.to_string_lossy().into_owned();
        }
    }

    // ---------------- dialogs ----------------

    fn dlg_add(&mut self, ctx: &egui::Context) {
        if !self.add_dlg.open { return; }
        let mut open = self.add_dlg.open;
        let mut cancel = false;
        let mut do_create = false;
        egui::Window::new("Archive name and parameters")
            .open(&mut open).resizable(false).collapsible(false)
            .show(ctx, |ui| {
                let d = &mut self.add_dlg;
                egui::Grid::new("addgrid").num_columns(2).show(ui, |ui| {
                    ui.label("Archive name:");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut d.name);
                        if ui.button("Browse…").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter("Archive",
                                    &["zip", "tar", "gz", "bz2", "xz"])
                                .save_file() {
                                d.name = p.to_string_lossy().into_owned();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Format:");
                    egui::ComboBox::from_id_salt("fmt")
                        .selected_text(["ZIP", "RAR", "TAR", "TAR.GZ",
                                        "TAR.BZ2", "TAR.XZ", "TAR.ZST"]
                                        [d.format_idx])
                        .show_ui(ui, |ui| {
                            for (i, f) in ["ZIP", "RAR", "TAR", "TAR.GZ",
                                           "TAR.BZ2", "TAR.XZ", "TAR.ZST"]
                                           .iter().enumerate() {
                                ui.selectable_value(&mut d.format_idx, i, *f);
                            }
                        });
                    ui.end_row();
                    ui.label("Compression:");
                    egui::ComboBox::from_id_salt("lvl")
                        .selected_text(engine::LEVELS[d.level_idx].0)
                        .show_ui(ui, |ui| {
                            for (i, (n, _)) in engine::LEVELS.iter().enumerate() {
                                ui.selectable_value(&mut d.level_idx, i, *n);
                            }
                        });
                    ui.end_row();
                    match d.format_idx {
                        0 => {
                            ui.label("Password (AES-256):");
                            ui.add(egui::TextEdit::singleline(&mut d.password)
                                .password(true));
                            ui.end_row();
                            ui.label("Comment:");
                            ui.text_edit_singleline(&mut d.comment);
                            ui.end_row();
                            ui.label("");
                            ui.checkbox(&mut d.make_sfx,
                                "Create self-extracting archive (SFX .exe)");
                            ui.end_row();
                        }
                        1 => {
                            ui.label("Password:");
                            ui.add(egui::TextEdit::singleline(&mut d.password)
                                .password(true));
                            ui.end_row();
                            ui.label("Dictionary:");
                            egui::ComboBox::from_id_salt("dict")
                                .selected_text(engine::RAR_DICTS[d.dict_idx])
                                .show_ui(ui, |ui| {
                                    for (i, n) in engine::RAR_DICTS.iter()
                                        .enumerate() {
                                        ui.selectable_value(&mut d.dict_idx,
                                                            i, *n);
                                    }
                                });
                            ui.end_row();
                            ui.label("Volume size:");
                            ui.text_edit_singleline(&mut d.volume);
                            ui.end_row();
                            ui.label("");
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut d.solid, "Solid");
                                ui.checkbox(&mut d.recovery,
                                            "Recovery record");
                                ui.checkbox(&mut d.lock, "Lock");
                            });
                            ui.end_row();
                        }
                        _ => {}
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() { do_create = true; }
                    if ui.button("Cancel").clicked() { cancel = true; }
                });
                ui.small("RAR writing uses the installed WinRAR Rar.exe.");
            });
        if cancel { open = false; }
        self.add_dlg.open = open;
        if do_create {
            let d = std::mem::take(&mut self.add_dlg);
            const EXTS: [&str; 7] = ["zip", "rar", "tar", "tar.gz",
                                     "tar.bz2", "tar.xz", "tar.zst"];
            let ext = EXTS[d.format_idx];
            let mut out = PathBuf::from(&d.name);
            // normalize the extension to the chosen format
            {
                let n = d.name.to_lowercase();
                for e in &["tar.gz", "tar.bz2", "tar.xz", "tar.zst",
                           "zip", "rar", "tar", "gz", "bz2", "xz", "zst",
                           "exe", "7z"] {
                    if n.ends_with(&format!(".{e}")) {
                        out.set_file_name(&n[..n.len() - e.len() - 1]);
                        break;
                    }
                }
                let base = out.to_string_lossy().into_owned();
                out = PathBuf::from(format!("{base}.{ext}"));
            }
            let pwd = if d.password.is_empty() { None }
                      else { Some(d.password) };
            let cmt = if d.comment.is_empty() { None }
                      else { Some(d.comment) };
            let lvl = engine::LEVELS[d.level_idx].1;
            let fmt_idx = d.format_idx;
            let make_sfx = d.make_sfx;
            let ropts = engine::RarOpts {
                level: (d.level_idx as i64).min(5),
                password: pwd.clone(),
                solid: d.solid, recovery: d.recovery, lock: d.lock,
                dict: if d.dict_idx == 0 { None }
                      else { Some(engine::RAR_DICTS[d.dict_idx].into()) },
                volume: if d.volume.trim().is_empty() { None }
                        else { Some(d.volume) },
            };
            let files = d.files;
            self.run_bg("Creating archive…", move || {
                (|| -> Result<Msg, String> {
                    match fmt_idx {
                        0 => {
                            engine::zip_add(&out, &files, lvl,
                                            pwd.as_deref(), cmt.as_deref())
                                .map_err(|e| e.to_string())?;
                            if make_sfx {
                                let sfx = out.with_extension("exe");
                                engine::make_sfx(&out, &sfx)
                                    .map_err(|e| e.to_string())?;
                                let _ = std::fs::remove_file(&out);
                            }
                            Ok(Msg::Done("Archive created".into()))
                        }
                        1 => {
                            engine::rar_create(&out, &files, &ropts)
                                .map(Msg::Done).map_err(|e| e.to_string())
                        }
                        _ => {
                            let fmt = [Format::Tar, Format::Gz, Format::Bz2,
                                       Format::Xz, Format::Zst]
                                       [fmt_idx - 2];
                            engine::tar_create(&out, &files, fmt)
                                .map(|n| Msg::Done(
                                    format!("TAR created ({n} items)")))
                                .map_err(|e| e.to_string())
                        }
                    }
                })()
            });
        }
    }

    fn dlg_extract(&mut self, ctx: &egui::Context) {
        if !self.extract_dlg.open { return; }
        let mut open = self.extract_dlg.open;
        let mut cancel = false;
        let mut go = false;
        egui::Window::new("Extraction path and options")
            .open(&mut open).resizable(false).collapsible(false)
            .show(ctx, |ui| {
                let d = &mut self.extract_dlg;
                egui::Grid::new("xgrid").num_columns(2).show(ui, |ui| {
                    ui.label("Destination:");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut d.dest);
                        if ui.button("Browse…").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .pick_folder() {
                                d.dest = p.to_string_lossy().into_owned();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut d.password)
                        .password(true));
                    ui.end_row();
                });
                ui.label(if d.members.is_empty() {
                    "Extracting: all files".to_string()
                } else {
                    format!("Extracting: {} selected", d.members.len())
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() { go = true; }
                    if ui.button("Cancel").clicked() { cancel = true; }
                });
            });
        if cancel { open = false; }
        self.extract_dlg.open = open;
        if go {
            let d = std::mem::take(&mut self.extract_dlg);
            let arc = self.archive.clone().unwrap();
            let dest = PathBuf::from(d.dest);
            let members: Option<Vec<String>> =
                if d.members.is_empty() { None } else { Some(d.members) };
            let pwd = if d.password.is_empty() { None } else { Some(d.password) };
            self.run_bg("Extracting…", move || {
                engine::extract_archive(&arc, &dest, members.as_deref(),
                                        pwd.as_deref())
                    .map(|n| Msg::Done(format!("Extracted {n} item(s)")))
                    .map_err(|e| e.to_string())
            });
        }
    }

    fn dlg_info(&mut self, ctx: &egui::Context) {
        if !self.show_info { return; }
        let mut open = self.show_info;
        egui::Window::new("Archive information")
            .open(&mut open).resizable(false).collapsible(false)
            .show(ctx, |ui| {
                if let (Some(p), Some(i)) = (&self.archive, &self.info) {
                    let total: u64 = i.entries.iter().map(|e| e.size).sum();
                    let packed: u64 = i.entries.iter().map(|e| e.packed).sum();
                    let disk = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                    egui::Grid::new("infogrid").num_columns(2).show(ui, |ui| {
                        ui.label("Archive:"); ui.label(p.display().to_string()); ui.end_row();
                        ui.label("Format:"); ui.label(i.format.label()); ui.end_row();
                        ui.label("Entries:"); ui.label(i.entries.len().to_string()); ui.end_row();
                        ui.label("Unpacked size:"); ui.label(fmt_size(total)); ui.end_row();
                        ui.label("Packed size:"); ui.label(fmt_size(packed)); ui.end_row();
                        ui.label("File size:"); ui.label(fmt_size(disk)); ui.end_row();
                        if packed > 0 && total > 0 {
                            ui.label("Ratio:");
                            ui.label(format!("{}%", 100 - packed * 100 / total));
                            ui.end_row();
                        }
                        ui.label("Attributes:");
                        let mut flags = Vec::new();
                        if i.solid { flags.push("solid"); }
                        if i.locked { flags.push("locked"); }
                        if i.recovery { flags.push("recovery record"); }
                        if i.encrypted_headers { flags.push("encrypted headers"); }
                        if i.multipart { flags.push("multivolume"); }
                        if i.sfx { flags.push("SFX"); }
                        ui.label(if flags.is_empty() { "—".into() }
                                 else { flags.join(", ") });
                        ui.end_row();
                        if !i.comment.is_empty() {
                            ui.label("Comment:"); ui.label(&i.comment); ui.end_row();
                        }
                    });
                } else {
                    ui.label("No archive open");
                }
            });
        self.show_info = open;
    }

    fn dlg_comment(&mut self, ctx: &egui::Context) {
        if !self.show_comment { return; }
        let mut open = self.show_comment;
        let mut close = false;
        let mut save = false;
        egui::Window::new("Archive comment")
            .open(&mut open).resizable(true).collapsible(false)
            .show(ctx, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.comment_text)
                    .desired_width(400.0)
                    .min_size(egui::vec2(400.0, 140.0)));
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() { save = true; }
                    if ui.button("Close").clicked() { close = true; }
                });
            });
        if close { open = false; }
        self.show_comment = open;
        if save {
            if let Some(arc) = self.archive.clone() {
                let txt = self.comment_text.clone();
                self.run_bg("Saving comment…", move || {
                    engine::zip_set_comment(&arc, &txt)
                        .map(|_| Msg::Done("Comment saved".into()))
                        .map_err(|e| e.to_string())
                });
            }
        }
    }

    fn dlg_about(&mut self, ctx: &egui::Context) {
        if !self.show_about { return; }
        let mut open = self.show_about;
        egui::Window::new("About UnRAR")
            .open(&mut open).resizable(false).collapsible(false)
            .show(ctx, |ui| {
                ui.heading("UnRAR 1.0");
                ui.label("An open-source WinRAR-style archive manager (MIT).");
                ui.label("ZIP / TAR / GZ / BZ2 / XZ / ZST / 7z / RAR read.");
                ui.label("Format detection by content signature.");
                ui.hyperlink_to("Source", "https://example.invalid/unrar");
            });
        self.show_about = open;
    }
}

// ---------------------------------------------------------------- eframe

impl eframe::App for UnRARApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // worker messages
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Msg::Done(s)) => {
                    self.busy = None;
                    self.set_status(s);
                    if let Some(p) = self.archive.clone() {
                        if let Ok(info) = engine::list_archive(&p) {
                            self.info = Some(info);
                        }
                    }
                    self.rx = None;
                }
                Ok(Msg::Err(e)) => {
                    self.busy = None;
                    self.set_status("Error");
                    rfd::MessageDialog::new().set_title(APP_TITLE)
                        .set_description(&e).show();
                    self.rx = None;
                }
                Ok(Msg::ViewData { name, data }) => {
                    self.busy = None;
                    self.viewers.push(Viewer::new(&name, data));
                    self.rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.busy = None;
                    self.rx = None;
                }
            }
        }

        // keyboard
        if ctx.input(|i| i.key_pressed(Key::O) && i.modifiers.ctrl) {
            self.cmd_open_dialog();
        }
        if ctx.input(|i| i.key_pressed(Key::E) && i.modifiers.ctrl) {
            self.cmd_extract();
        }
        if ctx.input(|i| i.key_pressed(Key::T) && i.modifiers.ctrl) {
            self.cmd_test();
        }
        if ctx.input(|i| i.key_pressed(Key::F) && i.modifiers.ctrl) {
            self.find_open = !self.find_open;
        }
        if ctx.input(|i| i.key_pressed(Key::I) && i.modifiers.ctrl) {
            self.show_info = true;
        }
        if ctx.input(|i| i.key_pressed(Key::A) && i.modifiers.ctrl) {
            for r in self.visible_rows() {
                self.selected.insert(r.full);
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) {
            self.cmd_view();
        }
        if ctx.input(|i| i.key_pressed(Key::Delete)) {
            self.cmd_delete();
        }
        if ctx.input(|i| i.key_pressed(Key::Backspace)) {
            self.go_up();
        }

        // drag & drop
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for df in dropped {
            let path = df.path().to_path_buf();
            if !path.as_os_str().is_empty() {
                if self.in_archive()
                    && engine::detect_format(&self.archive.clone().unwrap())
                        == Format::Zip {
                    let arc = self.archive.clone().unwrap();
                    let lvl = engine::LEVELS[self.level_idx].1;
                    let files = vec![path];
                    self.run_bg("Adding…", move || {
                        engine::zip_add(&arc, &files, lvl, None, None)
                            .map(|n| Msg::Done(format!("Added {n} file(s)")))
                            .map_err(|e| e.to_string())
                    });
                } else {
                    self.try_open(path);
                }
            }
        }

        // menu bar
        egui::Panel::top("menubar").show(ui, |ui| {
            ui.horizontal(|ui| self.menubar(ui));
        });

        // toolbar
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| self.toolbar(ui));
        });

        // location bar
        egui::Panel::top("location").show(ui, |ui| {
            ui.horizontal(|ui| self.location_bar(ui));
        });

        // find bar
        if self.find_open {
            egui::Panel::bottom("findbar").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Find:");
                    ui.text_edit_singleline(&mut self.find_text);
                    if ui.button("x").clicked() {
                        self.find_open = false;
                        self.find_text.clear();
                    }
                });
            });
        }

        // status bar
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_left);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(&self.status_right);
                });
            });
            if let Some(b) = &self.busy {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(b);
                });
            }
        });

        // central list
        egui::CentralPanel::default_margins().show(ui, |ui| {
            if self.in_archive() {
                self.rows_archive(ui);
            } else {
                self.rows_fs(ui);
            }
        });

        // floating windows
        self.dlg_add(&ctx);
        self.dlg_extract(&ctx);
        self.dlg_info(&ctx);
        self.dlg_comment(&ctx);
        self.dlg_about(&ctx);
        for v in &mut self.viewers {
            v.ui(&ctx);
        }
        self.viewers.retain(|v| v.open);

        // keep repainting while a worker runs so status updates land
        if self.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}

pub fn run(start: Option<PathBuf>) -> eframe::Result<()> {
    let icon = crate::icons::app_icon();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 640.0])
            .with_min_inner_size([680.0, 340.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(APP_TITLE, options,
        Box::new(move |cc| Ok(Box::new(UnRARApp::new(cc, start)))))
}
