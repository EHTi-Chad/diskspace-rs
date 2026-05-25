use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::Disks;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
const SURFACE: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
const BORDER: Color32 = Color32::from_rgb(0x21, 0x26, 0x2d);
const BORDER_HOVER: Color32 = Color32::from_rgb(0x58, 0xa6, 0xff);
const TEXT: Color32 = Color32::from_rgb(0xe6, 0xed, 0xf3);
const TEXT_DIM: Color32 = Color32::from_rgb(0x7d, 0x85, 0x90);
const TEXT_DIMMER: Color32 = Color32::from_rgb(0x48, 0x4f, 0x58);
const ACCENT: Color32 = Color32::from_rgb(0x58, 0xa6, 0xff);
const GREEN: Color32 = Color32::from_rgb(0x3f, 0xb9, 0x50);
const YELLOW: Color32 = Color32::from_rgb(0xd2, 0x99, 0x22);
const RED: Color32 = Color32::from_rgb(0xf8, 0x51, 0x49);

const PIE_COLORS: &[Color32] = &[
    Color32::from_rgb(0x58, 0xa6, 0xff),
    Color32::from_rgb(0x3f, 0xb9, 0x50),
    Color32::from_rgb(0xd2, 0x99, 0x22),
    Color32::from_rgb(0xf8, 0x51, 0x49),
    Color32::from_rgb(0xbc, 0x8c, 0xff),
    Color32::from_rgb(0xff, 0x7b, 0x72),
    Color32::from_rgb(0x79, 0xc0, 0xff),
    Color32::from_rgb(0x56, 0xd3, 0x64),
    Color32::from_rgb(0xff, 0xa6, 0x57),
    Color32::from_rgb(0x39, 0xd3, 0x53),
];

fn pct_color(pct: f64) -> Color32 {
    if pct >= 90.0 { RED } else if pct >= 75.0 { YELLOW } else { GREEN }
}

fn format_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.1} {}", v, UNITS[i])
}

// ── Disk info ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct DiskInfo {
    mount: String,
    device: String,
    fs_type: String,
    total: u64,
    used: u64,
    free: u64,
    pct: f64,
}

impl DiskInfo {
    fn load_all() -> Vec<DiskInfo> {
        let disks = Disks::new_with_refreshed_list();
        let skip_fs = ["squashfs", "tmpfs", "devtmpfs", "proc", "sysfs",
                       "cgroup", "cgroup2", "devfs", "overlay"];
        let mut result = Vec::new();
        for disk in disks.list() {
            let fs = disk.file_system().to_string_lossy().to_string();
            if skip_fs.iter().any(|s| fs.eq_ignore_ascii_case(s)) {
                continue;
            }
            let total = disk.total_space();
            let free = disk.available_space();
            let used = total.saturating_sub(free);
            let pct = if total > 0 { used as f64 / total as f64 * 100.0 } else { 0.0 };
            result.push(DiskInfo {
                mount: disk.mount_point().to_string_lossy().to_string(),
                device: disk.name().to_string_lossy().to_string(),
                fs_type: fs,
                total, used, free, pct,
            });
        }
        result
    }
}

// ── Scan result for pie chart ─────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct DirEntry {
    name: String,
    path: Option<PathBuf>,  // None = "[files]" pseudo-entry
    size: u64,
}

#[derive(Clone, Debug)]
enum ScanState {
    Idle,
    Scanning,
    Done(Vec<DirEntry>),
}

fn du(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(walker) = std::fs::read_dir(path) {
        for entry in walker.flatten() {
            let p = entry.path();
            if p.is_symlink() { continue; }
            if p.is_dir() {
                total += du(&p);
            } else if let Ok(meta) = std::fs::metadata(&p) {
                total += meta.len();
            }
        }
    }
    total
}

fn scan_dir(path: &Path) -> Vec<DirEntry> {
    let mut dirs: Vec<DirEntry> = Vec::new();
    let mut files_size = 0u64;

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') { continue; }
            if p.is_symlink() { continue; }
            if p.is_dir() {
                let sz = du(&p);
                if sz > 0 {
                    dirs.push(DirEntry { name, path: Some(p), size: sz });
                }
            } else if let Ok(meta) = std::fs::metadata(&p) {
                files_size += meta.len();
            }
        }
    }
    dirs.sort_by(|a, b| b.size.cmp(&a.size));
    if files_size > 0 {
        dirs.push(DirEntry { name: "[files]".into(), path: None, size: files_size });
    }
    dirs
}

// ── File browser entry ────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FileEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
    modified: String,
    file_type: String,
}

fn file_icon(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "py" => "🐍", "js" | "ts" => "📜", "sh" => "⚙",
        "txt" | "md" => "📄", "pdf" => "📕", "log" => "📋",
        "jpg" | "jpeg" | "png" | "gif" | "svg" => "🖼",
        "mp4" | "mkv" | "avi" => "🎬",
        "mp3" | "wav" | "flac" => "🎵",
        "zip" | "tar" | "gz" | "xz" => "🗜",
        "deb" | "rpm" => "📦",
        "c" | "cpp" | "h" | "rs" | "go" => "💻",
        "json" | "xml" | "yaml" | "yml" => "📋",
        "db" | "sqlite" => "🗄",
        _ => "📄",
    }
}

fn read_dir_entries(path: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    if let Ok(iter) = std::fs::read_dir(path) {
        for e in iter.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') { continue; }
            let p = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
            let is_dir = meta.is_dir();
            let size = if is_dir { 0 } else { meta.len() };
            let modified = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| {
                    let secs = d.as_secs();
                    let (y, mo, day, h, min) = epoch_to_parts(secs);
                    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, mo, day, h, min)
                })
                .unwrap_or_default();
            let file_type = if is_dir {
                "Directory".into()
            } else {
                name.rsplit('.').next().unwrap_or("").to_uppercase()
            };
            entries.push(FileEntry { name, path: p, is_dir, size, modified, file_type });
        }
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    entries
}

fn epoch_to_parts(secs: u64) -> (u64, u64, u64, u64, u64) {
    let min = (secs / 60) % 60;
    let hour = (secs / 3600) % 24;
    let days = secs / 86400;
    // Rough Gregorian approximation
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        year += 1;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [31u64, if leap {29} else {28}, 31,30,31,30,31,31,30,31,30,31];
    let mut month = 1u64;
    for &md in &month_days {
        if remaining < md { break; }
        remaining -= md;
        month += 1;
    }
    (year, month, remaining + 1, hour, min)
}

// ── Detail view state ─────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum DetailTab { Files, Breakdown, Details }

struct DetailView {
    disk: DiskInfo,
    tab: DetailTab,

    // Files tab
    current_path: PathBuf,
    file_entries: Vec<FileEntry>,
    breadcrumb: Vec<(String, PathBuf)>,

    // Breakdown tab
    pie_path: PathBuf,
    pie_path_stack: Vec<PathBuf>,
    scan_state: Arc<Mutex<ScanState>>,
    pie_entries: Vec<DirEntry>,
    pie_hover: Option<usize>,

    // Details tab — nothing extra needed, reads from disk
}

impl DetailView {
    fn new(disk: DiskInfo) -> Self {
        let root = PathBuf::from(&disk.mount);
        let file_entries = read_dir_entries(&root);
        let breadcrumb = vec![(disk.mount.clone(), root.clone())];
        let scan_state = Arc::new(Mutex::new(ScanState::Idle));
        let mut dv = Self {
            disk,
            tab: DetailTab::Files,
            current_path: root.clone(),
            file_entries,
            breadcrumb,
            pie_path: root.clone(),
            pie_path_stack: vec![root.clone()],
            scan_state,
            pie_entries: Vec::new(),
            pie_hover: None,
        };
        dv.start_scan(root);
        dv
    }

    fn navigate_files(&mut self, path: PathBuf) {
        self.current_path = path.clone();
        self.file_entries = read_dir_entries(&path);
        // Rebuild breadcrumb
        self.breadcrumb.clear();
        let root = PathBuf::from(&self.disk.mount);
        let mut parts = Vec::new();
        let mut p = path.clone();
        loop {
            let name = p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string());
            parts.push((name, p.clone()));
            if p == root { break; }
            if let Some(parent) = p.parent() {
                p = parent.to_path_buf();
            } else { break; }
        }
        parts.reverse();
        self.breadcrumb = parts;
    }

    fn start_scan(&mut self, path: PathBuf) {
        self.pie_path = path.clone();
        *self.scan_state.lock().unwrap() = ScanState::Scanning;
        self.pie_entries.clear();
        let state = Arc::clone(&self.scan_state);
        thread::spawn(move || {
            let results = scan_dir(&path);
            *state.lock().unwrap() = ScanState::Done(results);
        });
    }

    fn pie_navigate(&mut self, path: PathBuf) {
        self.pie_path_stack.push(path.clone());
        self.start_scan(path);
    }

    fn pie_navigate_to(&mut self, path: PathBuf) {
        // Truncate stack to this path
        if let Some(pos) = self.pie_path_stack.iter().position(|p| p == &path) {
            self.pie_path_stack.truncate(pos + 1);
        }
        self.start_scan(path);
    }

    fn poll_scan(&mut self) {
        let state = self.scan_state.lock().unwrap().clone();
        if let ScanState::Done(entries) = state {
            self.pie_entries = entries;
            *self.scan_state.lock().unwrap() = ScanState::Idle;
        }
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

struct DiskSpaceApp {
    disks: Vec<DiskInfo>,
    last_refresh: Instant,
    detail: Option<DetailView>,
    about_open: bool,
}

impl DiskSpaceApp {
    fn new(_cc: &eframe::CreationContext) -> Self {
        Self {
            disks: DiskInfo::load_all(),
            last_refresh: Instant::now(),
            detail: None,
            about_open: false,
        }
    }
}

impl eframe::App for DiskSpaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply dark theme
        let mut style = (*ctx.style()).clone();
        style.visuals.window_fill = BG;
        style.visuals.panel_fill = BG;
        style.visuals.override_text_color = Some(TEXT);
        ctx.set_style(style);

        // Auto-refresh every 30s
        if self.last_refresh.elapsed() > Duration::from_secs(30) && self.detail.is_none() {
            self.disks = DiskInfo::load_all();
            self.last_refresh = Instant::now();
        }

        // Poll background scans
        if let Some(ref mut dv) = self.detail {
            dv.poll_scan();
            let scanning = matches!(*dv.scan_state.lock().unwrap(), ScanState::Scanning);
            if scanning { ctx.request_repaint_after(Duration::from_millis(100)); }
        }

        if self.detail.is_some() {
            self.show_detail(ctx);
        } else {
            self.show_main(ctx);
        }
    }
}

impl DiskSpaceApp {
    fn show_main(&mut self, ctx: &egui::Context) {
        // Header panel
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::none().fill(SURFACE).inner_margin(egui::Margin::symmetric(24.0, 14.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("DISKSPACE").size(20.0).strong().color(TEXT));
                        ui.label(RichText::new("STORAGE MONITOR  —  click a drive to explore")
                            .size(11.0).color(TEXT_DIM));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("⟳  Refresh").color(ACCENT)).clicked() {
                            self.disks = DiskInfo::load_all();
                            self.last_refresh = Instant::now();
                        }
                    });
                });
            });

        // Footer
        egui::TopBottomPanel::bottom("footer")
            .frame(egui::Frame::none().fill(SURFACE).inner_margin(egui::Margin::symmetric(24.0, 8.0)))
            .show(ctx, |ui| {
                let total_used: u64 = self.disks.iter().map(|d| d.used).sum();
                let total_size: u64 = self.disks.iter().map(|d| d.total).sum();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Auto-refresh: 30s").size(10.0).color(TEXT_DIMMER));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(
                            RichText::new("About").size(10.0).color(TEXT_DIM)
                        ).frame(false)).clicked() {
                            self.about_open = true;
                        }
                        ui.separator();
                        ui.label(RichText::new(
                            format!("Total: {} used of {}", format_bytes(total_used), format_bytes(total_size))
                        ).size(11.0).color(TEXT_DIM));
                    });
                });
            });

        // About window
        if self.about_open {
            egui::Window::new("About")
                .collapsible(false)
                .resizable(false)
                .fixed_size([280.0, 130.0])
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::none()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(egui::Margin::same(24.0))
                    .rounding(Rounding::same(8.0)))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("DiskSpace Analyzer").size(16.0).strong().color(TEXT));
                        ui.add_space(6.0);
                        ui.label(RichText::new("EHTi Copyright 2026").size(12.0).color(TEXT_DIM));
                        ui.add_space(4.0);
                        ui.label(RichText::new("With help from Claude :)").size(11.0).color(TEXT_DIMMER));
                        ui.add_space(16.0);
                        if ui.button(RichText::new("  Close  ").color(ACCENT)).clicked() {
                            self.about_open = false;
                        }
                    });
                });
        }

        // Disk cards
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::symmetric(18.0, 12.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let disks = self.disks.clone();
                    let mut open_idx: Option<usize> = None;
                    for (i, disk) in disks.iter().enumerate() {
                        ui.add_space(6.0);
                        let clicked = drive_card(ui, disk);
                        if clicked { open_idx = Some(i); }
                        ui.add_space(6.0);
                    }
                    if let Some(i) = open_idx {
                        self.detail = Some(DetailView::new(self.disks[i].clone()));
                    }
                });
            });
    }

    fn show_detail(&mut self, ctx: &egui::Context) {
        let dv = self.detail.as_mut().unwrap();

        // Header
        let mut go_back = false;
        egui::TopBottomPanel::top("detail_header")
            .frame(egui::Frame::none().fill(SURFACE).inner_margin(egui::Margin::symmetric(20.0, 12.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("← Back").color(TEXT_DIM)).clicked() {
                        go_back = true;
                    }
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&dv.disk.mount).size(15.0).strong().color(TEXT));
                        ui.label(RichText::new(format!(
                            "{}  •  {}  •  {} / {}",
                            dv.disk.device, dv.disk.fs_type,
                            format_bytes(dv.disk.used), format_bytes(dv.disk.total)
                        )).size(11.0).color(TEXT_DIM));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let pct = dv.disk.pct;
                        ui.label(RichText::new(format!("{:.0}%", pct))
                            .size(16.0).strong().color(pct_color(pct)));
                        ui.add_space(8.0);
                        // Mini bar
                        let (resp, painter) = ui.allocate_painter(Vec2::new(120.0, 10.0), Sense::hover());
                        let r = resp.rect;
                        painter.rect_filled(r, Rounding::same(4.0), BORDER);
                        let fill_w = r.width() * pct as f32 / 100.0;
                        painter.rect_filled(
                            Rect::from_min_size(r.min, Vec2::new(fill_w, r.height())),
                            Rounding::same(4.0), pct_color(pct)
                        );
                    });
                });
            });

        if go_back {
            self.detail = None;
            return;
        }

        let dv = self.detail.as_mut().unwrap();

        // Tab bar
        egui::TopBottomPanel::top("detail_tabs")
            .frame(egui::Frame::none().fill(SURFACE))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    for (label, tab) in [
                        ("  Files  ", DetailTab::Files),
                        ("  Breakdown  ", DetailTab::Breakdown),
                        ("  Details  ", DetailTab::Details),
                    ] {
                        let active = dv.tab == tab;
                        let color = if active { ACCENT } else { TEXT_DIM };
                        let resp = ui.add(egui::Button::new(
                            RichText::new(label).size(12.0).color(color)
                        ).frame(false));
                        if active {
                            let r = resp.rect;
                            ui.painter().line_segment(
                                [Pos2::new(r.min.x, r.max.y), Pos2::new(r.max.x, r.max.y)],
                                Stroke::new(2.0, ACCENT)
                            );
                        }
                        if resp.clicked() { dv.tab = tab; }
                    }
                });
                ui.add_space(1.0);
            });

        // Tab content
        let tab = dv.tab.clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |ui| {
                match tab {
                    DetailTab::Files     => show_files_tab(ui, dv),
                    DetailTab::Breakdown => show_breakdown_tab(ui, ctx, dv),
                    DetailTab::Details   => show_details_tab(ui, dv),
                }
            });
    }
}

// ── Drive card widget ─────────────────────────────────────────────────────────

fn drive_card(ui: &mut egui::Ui, disk: &DiskInfo) -> bool {
    let desired = Vec2::new(ui.available_width(), 110.0);
    let (resp, painter) = ui.allocate_painter(desired, Sense::click());
    let r = resp.rect;

    let hovered = resp.hovered();
    let border_col = if hovered { BORDER_HOVER } else { BORDER };

    painter.rect(r, Rounding::same(8.0), SURFACE, Stroke::new(1.0, border_col));

    let pad = 18.0;
    let inner = r.shrink(pad);

    // Mount point
    painter.text(
        Pos2::new(inner.min.x, inner.min.y + 14.0),
        egui::Align2::LEFT_CENTER,
        &disk.mount,
        FontId::proportional(14.0),
        TEXT,
    );

    // Device
    painter.text(
        Pos2::new(inner.min.x, inner.min.y + 30.0),
        egui::Align2::LEFT_CENTER,
        &disk.device,
        FontId::proportional(11.0),
        TEXT_DIM,
    );

    // FS type badge
    let badge_x = inner.min.x + 160.0;
    let badge_rect = Rect::from_center_size(
        Pos2::new(badge_x, inner.min.y + 14.0),
        Vec2::new(50.0, 16.0),
    );
    painter.rect_filled(badge_rect, Rounding::same(4.0), BORDER);
    painter.text(
        badge_rect.center(),
        egui::Align2::CENTER_CENTER,
        &disk.fs_type.to_uppercase(),
        FontId::proportional(10.0),
        TEXT_DIMMER,
    );

    // Percent
    let pct = disk.pct;
    painter.text(
        Pos2::new(inner.max.x, inner.min.y + 14.0),
        egui::Align2::RIGHT_CENTER,
        format!("{:.0}%", pct),
        FontId::proportional(16.0),
        pct_color(pct),
    );

    // Bar
    let bar_y = inner.min.y + 48.0;
    let bar_rect = Rect::from_min_size(
        Pos2::new(inner.min.x, bar_y),
        Vec2::new(inner.width(), 10.0),
    );
    painter.rect_filled(bar_rect, Rounding::same(4.0), BORDER);
    let fill_w = bar_rect.width() * pct as f32 / 100.0;
    painter.rect_filled(
        Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_rect.height())),
        Rounding::same(4.0),
        pct_color(pct),
    );

    // Sizes
    let sz_y = inner.min.y + 72.0;
    painter.text(Pos2::new(inner.min.x, sz_y), egui::Align2::LEFT_CENTER,
        format!("Used: {}", format_bytes(disk.used)), FontId::proportional(12.0), TEXT_DIM);
    painter.text(Pos2::new(inner.center().x, sz_y), egui::Align2::CENTER_CENTER,
        format!("Free: {}", format_bytes(disk.free)), FontId::proportional(12.0), TEXT_DIM);
    painter.text(Pos2::new(inner.max.x, sz_y), egui::Align2::RIGHT_CENTER,
        format!("Total: {}", format_bytes(disk.total)), FontId::proportional(12.0), TEXT_DIM);

    // Hint
    painter.text(Pos2::new(inner.max.x, inner.min.y + 90.0), egui::Align2::RIGHT_CENTER,
        "click to explore →", FontId::proportional(10.0), TEXT_DIMMER);

    resp.clicked()
}

// ── Files tab ─────────────────────────────────────────────────────────────────

fn show_files_tab(ui: &mut egui::Ui, dv: &mut DetailView) {
    // Breadcrumb
    egui::TopBottomPanel::top("bc_files")
        .frame(egui::Frame::none().fill(SURFACE).inner_margin(egui::Margin::symmetric(16.0, 6.0)))
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let bc = dv.breadcrumb.clone();
                for (i, (name, path)) in bc.iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new(" / ").color(TEXT_DIMMER).size(12.0));
                    }
                    if ui.add(egui::Button::new(
                        RichText::new(name).color(ACCENT).size(12.0)
                    ).frame(false)).clicked() {
                        dv.navigate_files(path.clone());
                    }
                }
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(BG))
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Header row
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    for (label, width) in [("", 28.0), ("Name", 300.0), ("Size", 90.0), ("Type", 100.0), ("Modified", 130.0)] {
                        ui.add_sized([width, 20.0],
                            egui::Label::new(RichText::new(label).size(11.0).color(TEXT_DIM)));
                    }
                });
                ui.separator();

                let root = PathBuf::from(&dv.disk.mount);
                let mut nav_to: Option<PathBuf> = None;

                if dv.current_path != root {
                    let parent = dv.current_path.parent().map(|p| p.to_path_buf());
                    if let Some(p) = parent {
                        let row = ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.add_sized([28.0, 20.0], egui::Label::new("📁"));
                            ui.add_sized([300.0, 20.0],
                                egui::Label::new(RichText::new("..").color(TEXT_DIM).size(12.0)));
                        });
                        if row.response.interact(Sense::click()).clicked() {
                            nav_to = Some(p);
                        }
                    }
                }

                let entries = dv.file_entries.clone();
                for entry in &entries {
                    let row = ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        let icon = if entry.is_dir { "📁" } else { file_icon(&entry.name) };
                        ui.add_sized([28.0, 20.0], egui::Label::new(icon));
                        ui.add_sized([300.0, 20.0],
                            egui::Label::new(RichText::new(&entry.name).size(12.0).color(TEXT)));
                        let size_str = if entry.is_dir { "—".into() } else { format_bytes(entry.size) };
                        ui.add_sized([90.0, 20.0],
                            egui::Label::new(RichText::new(size_str).size(12.0).color(TEXT_DIM)));
                        ui.add_sized([100.0, 20.0],
                            egui::Label::new(RichText::new(&entry.file_type).size(12.0).color(TEXT_DIM)));
                        ui.add_sized([130.0, 20.0],
                            egui::Label::new(RichText::new(&entry.modified).size(12.0).color(TEXT_DIM)));
                    });
                    if entry.is_dir && row.response.interact(Sense::click()).clicked() {
                        nav_to = Some(entry.path.clone());
                    }
                    ui.separator();
                }

                if let Some(p) = nav_to {
                    dv.navigate_files(p);
                }
            });
        });
}

// ── Breakdown tab (pie chart) ─────────────────────────────────────────────────

fn show_breakdown_tab(ui: &mut egui::Ui, ctx: &egui::Context, dv: &mut DetailView) {
    // Breadcrumb
    let stack = dv.pie_path_stack.clone();
    let mut nav_to: Option<PathBuf> = None;

    egui::TopBottomPanel::top("bc_pie")
        .frame(egui::Frame::none().fill(SURFACE).inner_margin(egui::Margin::symmetric(16.0, 6.0)))
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, path) in stack.iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new(" / ").color(TEXT_DIMMER).size(12.0));
                    }
                    let name = path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.to_string_lossy().to_string());
                    if ui.add(egui::Button::new(
                        RichText::new(&name).color(ACCENT).size(12.0)
                    ).frame(false)).clicked() {
                        nav_to = Some(path.clone());
                    }
                }
            });
        });

    if let Some(p) = nav_to {
        dv.pie_navigate_to(p);
        return;
    }

    let scanning = matches!(*dv.scan_state.lock().unwrap(), ScanState::Scanning);

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(BG))
        .show_inside(ui, |ui| {
            if scanning {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Scanning…").size(14.0).color(TEXT_DIM));
                });
                return;
            }

            if dv.pie_entries.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Empty or inaccessible directory.")
                        .size(14.0).color(TEXT_DIM));
                });
                return;
            }

            let total: u64 = dv.pie_entries.iter().map(|e| e.size).sum();
            let entries: Vec<DirEntry> = dv.pie_entries.clone();
            let show_count = entries.len().min(16);
            let other_size: u64 = if entries.len() > 16 {
                entries[16..].iter().map(|e| e.size).sum()
            } else { 0 };

            // Left: pie chart | Right: legend
            let avail = ui.available_size();
            let pie_size = avail.y.min(avail.x * 0.65).min(460.0);

            let mut nav_to: Option<PathBuf> = None;

            ui.horizontal(|ui| {
                // Pie chart
                let (resp, painter) = ui.allocate_painter(
                    Vec2::splat(pie_size), Sense::click()
                );
                let rect = resp.rect;
                let cx = rect.center().x;
                let cy = rect.center().y;
                let r = pie_size * 0.42;
                let inner_r = r * 0.38;

                painter.rect_filled(rect, Rounding::ZERO, BG);

                // Draw slices
                let mut start_angle = -std::f32::consts::PI / 2.0;
                let mut hover_idx: Option<usize> = None;

                let mouse = ctx.input(|i| i.pointer.hover_pos());

                // First pass: find hover
                let mut angles: Vec<(f32, f32)> = Vec::new();
                for i in 0..show_count {
                    let frac = entries[i].size as f32 / total as f32;
                    let end_angle = start_angle + frac * 2.0 * std::f32::consts::PI;
                    angles.push((start_angle, end_angle));
                    if let Some(m) = mouse {
                        let dx = m.x - cx;
                        let dy = m.y - cy;
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist >= inner_r && dist <= r {
                            let mut a = dy.atan2(dx);
                            let s = start_angle;
                            let e = end_angle;
                            while a < s { a += 2.0 * std::f32::consts::PI; }
                            while a > e + 2.0 * std::f32::consts::PI { a -= 2.0 * std::f32::consts::PI; }
                            if a >= s && a <= e {
                                hover_idx = Some(i);
                            }
                        }
                    }
                    start_angle = end_angle;
                }
                if other_size > 0 {
                    angles.push((start_angle, start_angle + other_size as f32 / total as f32 * 2.0 * std::f32::consts::PI));
                }

                // Second pass: draw
                for (i, &(sa, ea)) in angles.iter().enumerate() {
                    let col = if i < show_count {
                        PIE_COLORS[i % PIE_COLORS.len()]
                    } else {
                        TEXT_DIMMER
                    };

                    let is_hover = hover_idx == Some(i);
                    let expand = if is_hover { 10.0f32 } else { 0.0 };
                    let mid = (sa + ea) / 2.0;
                    let ox = mid.cos() * expand;
                    let oy = mid.sin() * expand;

                    let steps = ((ea - sa).abs() / 0.02).ceil() as usize + 2;
                    let mut points = vec![Pos2::new(cx + ox, cy + oy)];
                    for s in 0..=steps {
                        let a = sa + (ea - sa) * s as f32 / steps as f32;
                        points.push(Pos2::new(cx + ox + a.cos() * r, cy + oy + a.sin() * r));
                    }
                    painter.add(egui::Shape::convex_polygon(points, col, Stroke::NONE));

                    painter.line_segment(
                        [Pos2::new(cx + ox, cy + oy),
                         Pos2::new(cx + ox + sa.cos() * r, cy + oy + sa.sin() * r)],
                        Stroke::new(2.0, BG),
                    );
                }

                // Donut hole
                painter.circle_filled(Pos2::new(cx, cy), inner_r, BG);

                // Center text
                let (center_text, center_sub, center_col) = if let Some(i) = hover_idx {
                    let e = &entries[i];
                    let pct = e.size as f64 / total as f64 * 100.0;
                    let col = PIE_COLORS[i % PIE_COLORS.len()];
                    (e.name.clone(), format!("{} ({:.1}%)", format_bytes(e.size), pct), col)
                } else {
                    (format_bytes(total), format!("{} items", show_count), TEXT_DIM)
                };

                painter.text(Pos2::new(cx, cy - 8.0), egui::Align2::CENTER_CENTER,
                    &center_text, FontId::proportional(12.0), center_col);
                painter.text(Pos2::new(cx, cy + 10.0), egui::Align2::CENTER_CENTER,
                    &center_sub, FontId::proportional(11.0), TEXT_DIM);
                if let Some(i) = hover_idx {
                    if entries[i].path.is_some() {
                        painter.text(Pos2::new(cx, cy + 26.0), egui::Align2::CENTER_CENTER,
                            "click to open", FontId::proportional(10.0), ACCENT);
                    }
                }

                // Handle click — store nav target, apply after closure
                if resp.clicked() {
                    if let Some(i) = hover_idx {
                        if let Some(ref path) = entries[i].path {
                            nav_to = Some(path.clone());
                        }
                    }
                }

                // Legend
                ui.add_space(8.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_min_width(200.0);

                    for (i, entry) in entries.iter().take(show_count).enumerate() {
                        let col = PIE_COLORS[i % PIE_COLORS.len()];
                        let is_dir = entry.path.is_some();
                        ui.horizontal(|ui| {
                            // Colour dot
                            let (dot_resp, dot_painter) = ui.allocate_painter(Vec2::splat(14.0), Sense::hover());
                            dot_painter.circle_filled(dot_resp.rect.center(), 6.0, col);
                            let name_text = RichText::new(&entry.name).size(12.0)
                                .color(if is_dir { ACCENT } else { TEXT });
                            let name_resp = ui.add(egui::Label::new(name_text).sense(Sense::click()));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(format_bytes(entry.size)).size(12.0).color(TEXT_DIM));
                            });
                            if is_dir && name_resp.clicked() {
                                if let Some(ref p) = entry.path {
                                    nav_to = Some(p.clone());
                                }
                            }
                        });
                        ui.add_space(4.0);
                    }
                    if other_size > 0 {
                        ui.horizontal(|ui| {
                            let (dot_resp, dot_painter) = ui.allocate_painter(Vec2::splat(14.0), Sense::hover());
                            dot_painter.circle_filled(dot_resp.rect.center(), 6.0, TEXT_DIMMER);
                            ui.label(RichText::new("(other)").size(12.0).color(TEXT_DIM));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(format_bytes(other_size)).size(12.0).color(TEXT_DIM));
                            });
                        });
                    }
                });
            });

            // Apply navigation after closures release borrows
            if let Some(path) = nav_to {
                dv.pie_navigate(path);
            }
        });
}

// ── Details tab ───────────────────────────────────────────────────────────────

fn show_details_tab(ui: &mut egui::Ui, dv: &DetailView) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);
        let d = &dv.disk;

        section_header(ui, "STORAGE");
        detail_row(ui, "Mount Point", &d.mount, None);
        detail_row(ui, "Device", &d.device, None);
        detail_row(ui, "Filesystem", &d.fs_type.to_uppercase(), None);
        detail_row(ui, "Total Capacity", &format_bytes(d.total), None);
        detail_row(ui, "Used Space", &format_bytes(d.used), Some(pct_color(d.pct)));
        detail_row(ui, "Free Space", &format_bytes(d.free), None);
        detail_row(ui, "Usage", &format!("{:.1}%", d.pct), Some(pct_color(d.pct)));

        section_header(ui, "I/O STATISTICS");
        // sysinfo doesn't expose I/O per-disk easily; show what we can
        detail_row(ui, "Note", "Run with sudo for full I/O stats", Some(TEXT_DIM));

        section_header(ui, "SMART / HEALTH");
        // Try smartctl
        match std::process::Command::new("smartctl")
            .args(["-H", "-A", &d.device])
            .output()
        {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                if text.contains("PASSED") {
                    detail_row(ui, "Overall Health", "PASSED", Some(GREEN));
                } else if text.contains("FAILED") {
                    detail_row(ui, "Overall Health", "FAILED!", Some(RED));
                } else {
                    detail_row(ui, "Overall Health", "Unknown / Not Supported", None);
                }
                let key_attrs: HashMap<&str, &str> = [
                    ("5", "Reallocated Sectors"),
                    ("9", "Power-On Hours"),
                    ("187", "Uncorrectable Errors"),
                    ("194", "Temperature (C)"),
                    ("197", "Pending Sectors"),
                ].iter().cloned().collect();
                for line in text.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 10 {
                        if let Some(&label) = key_attrs.get(parts[0]) {
                            detail_row(ui, label, parts[9], None);
                        }
                    }
                }
            }
            Err(_) => {
                detail_row(ui, "smartctl", "Not installed (sudo apt install smartmontools)", Some(TEXT_DIM));
            }
        }

        section_header(ui, "SYSTEM");
        if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
            detail_row(ui, "Hostname", hostname.trim(), None);
        }
        if let Ok(os) = std::fs::read_to_string("/etc/os-release") {
            for line in os.lines() {
                if line.starts_with("PRETTY_NAME=") {
                    let name = line.trim_start_matches("PRETTY_NAME=").trim_matches('"');
                    detail_row(ui, "OS", name, None);
                    break;
                }
            }
        }
    });
}

fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.label(RichText::new(title).size(11.0).color(TEXT_DIMMER).strong());
    });
    ui.add_space(4.0);
}

fn detail_row(ui: &mut egui::Ui, key: &str, value: &str, color: Option<Color32>) {
    let col = color.unwrap_or(TEXT);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.add_sized([200.0, 20.0],
            egui::Label::new(RichText::new(key).size(12.0).color(TEXT_DIM)));
        ui.label(RichText::new(value).size(12.0).color(col));
    });
    ui.separator();
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DiskSpace")
            .with_inner_size([700.0, 580.0])
            .with_min_inner_size([500.0, 400.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../ehti.ico"))
                    .unwrap_or_default()
            ),
        ..Default::default()
    };
    eframe::run_native(
        "DiskSpace",
        options,
        Box::new(|cc| Box::new(DiskSpaceApp::new(cc))),
    )
}
