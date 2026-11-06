use crate::{
    platform,
    scan::{self, Event, Index, Job, Node, Profile, Stats},
};
use eframe::egui::*;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const PAPER: Color32 = Color32::from_rgb(251, 250, 247);
const PANEL: Color32 = Color32::from_rgb(243, 243, 238);
const INK: Color32 = Color32::from_rgb(53, 65, 62);
const SUB: Color32 = Color32::from_rgb(103, 114, 106);
const RULE: Color32 = Color32::from_rgb(222, 225, 216);
const ACCENT: Color32 = Color32::from_rgb(82, 113, 94);
const MINT: Color32 = Color32::from_rgb(224, 233, 223);
const COLORS: [Color32; 6] = [
    Color32::from_rgb(135, 171, 153),
    Color32::from_rgb(145, 170, 189),
    Color32::from_rgb(197, 163, 140),
    Color32::from_rgb(166, 156, 182),
    Color32::from_rgb(195, 183, 131),
    Color32::from_rgb(167, 178, 176),
];
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
enum Mode {
    #[default]
    Grid,
    Ring,
    Both,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    mode: Mode,
    profile: Profile,
}
#[derive(Clone)]
struct Branch {
    node: Node,
    children: Vec<Branch>,
    color: Color32,
}
pub struct App {
    settings: Settings,
    job: Option<Job>,
    index: Option<Index>,
    root_path: Option<PathBuf>,
    stats: Stats,
    active: bool,
    paused: bool,
    current: i64,
    current_node: Option<Node>,
    breadcrumbs: Vec<Node>,
    chart: Vec<Branch>,
    rows: Vec<Node>,
    page: usize,
    large: bool,
    selected: Option<Node>,
    detail_open: bool,
    hover: Option<i64>,
    notice: Option<String>,
    issues_open: bool,
    issue_rows: Vec<(String, String)>,
    last_refresh: Instant,
    space: Option<(u64, u64)>,
}
pub fn size(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < 5 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[unit])
    }
}
fn text(
    ui: &Ui,
    position: Pos2,
    anchor: Align2,
    value: impl ToString,
    points: f32,
    color: Color32,
) {
    ui.painter().text(
        position,
        anchor,
        value.to_string(),
        FontId::proportional(points),
        color,
    );
}
fn pill(ui: &mut Ui, label: &str, active: bool) -> Response {
    ui.add(
        Button::new(
            RichText::new(label)
                .size(13.0)
                .color(if active { ACCENT } else { SUB }),
        )
        .fill(if active { MINT } else { Color32::TRANSPARENT })
        .corner_radius(7)
        .min_size(vec2(65.0, 30.0)),
    )
}
fn mode_button(ui: &mut Ui, mode: Mode, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(70.0, 30.0), Sense::click());
    if active || response.hovered() {
        ui.painter()
            .rect_filled(rect, 7, if active { MINT } else { PANEL });
    }
    let center = rect.left_center() + vec2(15.0, 0.0);
    let color = if active { ACCENT } else { SUB };
    let icon = Rect::from_center_size(center, vec2(13.0, 13.0));
    match mode {
        Mode::Grid => {
            ui.painter()
                .rect_stroke(icon, 2, Stroke::new(1.2_f32, color), StrokeKind::Inside);
            ui.painter()
                .vline(center.x - 1.0, icon.y_range(), Stroke::new(1.0_f32, color));
            ui.painter().line_segment(
                [
                    center - vec2(0.0, 1.0),
                    icon.right_center() - vec2(0.0, 1.0),
                ],
                Stroke::new(1.0_f32, color),
            );
        }
        Mode::Ring => {
            ui.painter()
                .circle_stroke(center, 6.0, Stroke::new(1.4_f32, color));
            ui.painter()
                .circle_stroke(center, 3.0, Stroke::new(1.0_f32, color));
        }
        Mode::Both => {
            ui.painter()
                .rect_stroke(icon, 2, Stroke::new(1.2_f32, color), StrokeKind::Inside);
            ui.painter()
                .vline(center.x, icon.y_range(), Stroke::new(1.0_f32, color));
        }
    }
    text(
        ui,
        rect.left_center() + vec2(28.0, 0.0),
        Align2::LEFT_CENTER,
        match mode {
            Mode::Grid => "网格",
            Mode::Ring => "环形",
            Mode::Both => "对比",
        },
        12.0,
        color,
    );
    response
}
fn folder_icon(ui: &Ui, rect: Rect, color: Color32, directory: bool) {
    let center = rect.center();
    let r = Rect::from_center_size(center, vec2(17.0, 13.0));
    ui.painter()
        .rect_stroke(r, 2, Stroke::new(1.3_f32, color), StrokeKind::Inside);
    if directory {
        ui.painter().line_segment(
            [
                r.left_top() + vec2(2.0, 0.0),
                r.left_top() + vec2(2.0, -3.0),
            ],
            Stroke::new(1.3_f32, color),
        );
        ui.painter().line_segment(
            [
                r.left_top() + vec2(2.0, -3.0),
                r.left_top() + vec2(8.0, -3.0),
            ],
            Stroke::new(1.3_f32, color),
        );
    } else {
        ui.painter().line_segment(
            [center + vec2(-4.0, 0.0), center + vec2(4.0, 0.0)],
            Stroke::new(1.0_f32, color),
        );
    }
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let mut fonts = FontDefinitions::default();
        for (key, path) in [
            ("segoe", "C:\\Windows\\Fonts\\segoeui.ttf"),
            ("chinese", "C:\\Windows\\Fonts\\msyh.ttc"),
        ] {
            if let Ok(data) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert(key.into(), FontData::from_owned(data).into());
                fonts
                    .families
                    .entry(FontFamily::Proportional)
                    .or_default()
                    .push(key.into());
            }
        }
        // Use the platform UI font first and the Chinese font as a fallback.
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional)
            && family.iter().any(|s| s == "segoe")
        {
            family.retain(|s| s != "segoe");
            family.insert(0, "segoe".into());
        }
        cc.egui_ctx.set_fonts(fonts);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = Visuals::light();
        style.visuals.override_text_color = Some(INK);
        style.visuals.panel_fill = PAPER;
        style.visuals.window_fill = PAPER;
        style.visuals.selection.bg_fill = MINT;
        style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        style.spacing.item_spacing = vec2(9.0, 8.0);
        cc.egui_ctx.set_style(style);
        let settings = std::fs::read(scan::cache_root().join("settings.json"))
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default();
        let mut app = Self {
            settings,
            job: None,
            index: None,
            root_path: None,
            stats: Stats::default(),
            active: false,
            paused: false,
            current: 1,
            current_node: None,
            breadcrumbs: vec![],
            chart: vec![],
            rows: vec![],
            page: 0,
            large: false,
            selected: None,
            detail_open: false,
            hover: None,
            notice: None,
            issues_open: false,
            issue_rows: vec![],
            last_refresh: Instant::now(),
            space: None,
        };
        if let Some(path) = initial {
            app.begin(path);
        }
        app
    }
    fn save_preferences(&self) {
        let root = scan::cache_root();
        if std::fs::create_dir_all(&root).is_ok()
            && let Ok(data) = serde_json::to_vec(&self.settings)
        {
            let temp = root.join(format!("settings-{}.tmp", std::process::id()));
            if std::fs::write(&temp, data).is_ok() {
                let _ = std::fs::rename(temp, root.join("settings.json"));
            }
        }
    }
    fn begin(&mut self, path: PathBuf) {
        if self.active {
            self.notice = Some("请先停止当前扫描".into());
            return;
        }
        match scan::start(path.clone(), self.settings.profile) {
            Ok(job) => {
                // Drop the reader before deleting the previous run's private cache.
                self.index = None;
                if let Some(old) = self.job.take() {
                    let _ = old.worker.join();
                    for suffix in ["", "-wal", "-shm"] {
                        let _ = std::fs::remove_file(format!("{}{suffix}", old.path.display()));
                    }
                }
                self.index = Index::open(&job.path).ok();
                self.job = Some(job);
                self.root_path = Some(path.clone());
                self.space = platform::space(&path);
                self.current = 1;
                self.page = 0;
                self.large = false;
                self.stats = Stats::default();
                self.active = true;
                self.paused = false;
                self.selected = None;
                self.detail_open = false;
                self.notice = None;
                self.refresh();
            }
            Err(error) => self.notice = Some(error.to_string()),
        }
    }
    fn refresh(&mut self) {
        let Some(index) = &self.index else {
            return;
        };
        let result = index.snapshot(|index| {
            let current = index.get(self.current)?;
            let rows = index.list(self.current, self.large, self.page * scan::PAGE)?;
            let mut breadcrumbs = vec![current.clone()];
            let mut parent = current.parent;
            while let Some(id) = parent {
                if breadcrumbs.len() > 256 {
                    break;
                }
                let node = index.get(id)?;
                parent = node.parent;
                breadcrumbs.push(node);
            }
            breadcrumbs.reverse();
            let mut budget = 170;
            let chart = Self::branches(index, self.current, 3, &mut budget, None)?;
            Ok((current, rows, breadcrumbs, chart))
        });
        match result {
            Ok((node, rows, breadcrumbs, chart)) => {
                self.current_node = Some(node);
                self.rows = rows;
                self.breadcrumbs = breadcrumbs;
                self.chart = chart;
            }
            // A brief writer lock is retried on the next refresh.
            Err(error) => {
                if !self.active {
                    self.notice = Some(format!("读取扫描索引失败。{error}"));
                }
            }
        }
        self.last_refresh = Instant::now();
    }
    fn branches(
        index: &Index,
        id: i64,
        depth: usize,
        budget: &mut usize,
        parent_color: Option<Color32>,
    ) -> std::result::Result<Vec<Branch>, Box<dyn std::error::Error + Send + Sync>> {
        if depth == 0 || *budget == 0 {
            return Ok(vec![]);
        }
        let total = index.get(id)?;
        let mut nodes = index.chart(id)?;
        let limit = if depth == 3 { 48 } else { 10 };
        nodes.truncate(limit.min(budget.saturating_sub(1)));
        let sum = nodes.iter().map(|n| n.bytes).sum::<u64>();
        if total.bytes > sum {
            nodes.push(Node {
                id: -id,
                parent: Some(id),
                name: "其他项".into(),
                directory: true,
                bytes: total.bytes - sum,
                logical: 0,
                files: 0,
                folders: 0,
                status: 4,
            });
        }
        *budget = budget.saturating_sub(nodes.len());
        let mut output = vec![];
        for (i, node) in nodes.into_iter().enumerate() {
            let color = parent_color.unwrap_or(COLORS[i % COLORS.len()]);
            // Reserve room for peer branches before expanding another depth.
            let children = if node.directory && node.id > 0 && depth > 1 && *budget > 10 {
                Self::branches(
                    index,
                    node.id,
                    depth - 1,
                    budget,
                    Some(color.lerp_to_gamma(PAPER, 0.20)),
                )?
            } else {
                vec![]
            };
            output.push(Branch {
                node,
                children,
                color,
            });
        }
        Ok(output)
    }
    fn poll(&mut self, ctx: &Context) {
        let events = self
            .job
            .as_ref()
            .map(|j| j.events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Progress(stats) => self.stats = stats,
                Event::Finished(stats, error) => {
                    self.stats = stats;
                    self.active = false;
                    self.paused = false;
                    if let Some(error) = error {
                        self.notice = Some(error);
                    }
                    self.refresh();
                }
            }
        }
        if self.active {
            if self.last_refresh.elapsed() >= Duration::from_millis(650) {
                self.refresh();
            }
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
    fn navigate(&mut self, node: &Node) {
        if node.id < 0 {
            self.page = 0;
            self.settings.mode = Mode::Grid;
            self.refresh();
            return;
        }
        if node.directory {
            self.current = node.id;
            self.page = 0;
            self.large = false;
            self.refresh();
        } else {
            self.selected = Some(node.clone());
            self.detail_open = true;
        }
    }
    fn heading(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(31.0, 31.0), Sense::hover());
            ui.painter().rect_filled(rect, 8, ACCENT);
            for radius in [5.0, 9.0, 13.0] {
                ui.painter()
                    .circle_stroke(rect.center(), radius, Stroke::new(1.0_f32, MINT));
            }
            ui.label(RichText::new("盘屿").size(20.0).strong());
            ui.add_space((ui.available_width() / 2.0 - 180.0).max(8.0));
            if pill(ui, "空间总览", !self.large).clicked() {
                self.large = false;
                self.page = 0;
                self.refresh();
            }
            if pill(ui, "大文件", self.large).clicked() {
                self.large = true;
                self.page = 0;
                self.refresh();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(!self.active, Button::new("选择文件夹").frame(false))
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_title("选择要分析的文件夹")
                        .pick_folder()
                {
                    self.begin(path);
                }
            });
        });
    }
    fn disk_header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
            ui.painter().rect_filled(rect, 13, MINT);
            let r = rect.shrink(11.0);
            ui.painter().rect_filled(r, 5, ACCENT);
            ui.painter().line_segment(
                [
                    r.left_top() + vec2(4.0, 6.0),
                    r.right_top() + vec2(-4.0, 6.0),
                ],
                Stroke::new(2.0_f32, MINT),
            );
            ui.add_space(5.0);
            let path = self
                .root_path
                .as_ref()
                .map(|p| display_path(p))
                .unwrap_or_default();
            let name = self
                .root_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("本地磁盘 {}", path));
            let title_width = (ui.available_width() - 210.0).max(150.0);
            ui.allocate_ui_with_layout(
                vec2(title_width, 52.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.label(RichText::new(&name).size(25.0).strong());
                    if let Some((total, used)) = self.space {
                        ui.add(
                            Label::new(
                                RichText::new(format!(
                                    "{}   ·   已用 {} / {}",
                                    path,
                                    size(used),
                                    size(total)
                                ))
                                .size(12.0)
                                .color(SUB),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&path);
                    } else {
                        ui.add(Label::new(RichText::new(&path).size(12.0).color(SUB)).truncate())
                            .on_hover_text(&path);
                    }
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(!self.active, Button::new("重新扫描").corner_radius(8))
                    .clicked()
                    && let Some(path) = self.root_path.clone()
                {
                    self.begin(path);
                }
                ui.add_enabled_ui(!self.active, |ui| {
                    ui.menu_button("切换磁盘", |ui| {
                        for path in platform::drives() {
                            if ui.button(path.display().to_string()).clicked() {
                                ui.close();
                                self.begin(path);
                            }
                        }
                    });
                });
            });
        });
        ui.add_space(17.0);
        ui.separator();
        ui.add_space(6.0);
    }
    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            let enabled = self.current_node.as_ref().and_then(|n| n.parent);
            if ui
                .add_enabled(enabled.is_some(), Button::new("↑").frame(false))
                .on_hover_text("上一级")
                .clicked()
                && let Some(id) = enabled
            {
                self.current = id;
                self.page = 0;
                self.refresh();
            }
            for node in self.breadcrumbs.clone() {
                let name = if node.id == 1 {
                    "根目录"
                } else {
                    &node.name
                };
                if ui
                    .add(Button::new(RichText::new(name).size(12.0).color(SUB)).frame(false))
                    .clicked()
                {
                    self.navigate(&node);
                }
                ui.label(RichText::new("›").color(SUB));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for mode in [Mode::Both, Mode::Ring, Mode::Grid] {
                    if mode_button(ui, mode, self.settings.mode == mode).clicked() {
                        self.settings.mode = mode;
                        self.save_preferences();
                    }
                }
            });
        });
        ui.add_space(10.0);
    }
    fn empty(&mut self, ui: &mut Ui) {
        ui.add_space(65.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("空间，都看得见。").size(32.0).strong());
            ui.add_space(28.0);
            for path in platform::drives() {
                let space = platform::space(&path);
                let title = space
                    .map(|(total, used)| {
                        format!("{}    {} / {}", path.display(), size(used), size(total))
                    })
                    .unwrap_or_else(|| path.display().to_string());
                if ui
                    .add_sized(
                        [340.0, 54.0],
                        Button::new(RichText::new(title).size(16.0))
                            .fill(PANEL)
                            .corner_radius(12),
                    )
                    .clicked()
                {
                    self.begin(path);
                }
                ui.add_space(7.0);
            }
            ui.add_space(8.0);
            if ui.button("选择文件夹").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.begin(path);
            }
        });
    }
    fn map(&mut self, ui: &mut Ui, ring: bool, height: f32) {
        Frame::new()
            .fill(PANEL)
            .stroke(Stroke::new(1.0_f32, RULE))
            .corner_radius(14)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(if ring { "环形图" } else { "矩形网格" })
                        .size(15.0)
                        .strong(),
                );
                ui.add_space(9.0);
                let (rect, _) = ui
                    .allocate_exact_size(vec2(ui.available_width(), height - 65.0), Sense::hover());
                if self.chart.iter().all(|b| b.node.bytes == 0) {
                    text(
                        ui,
                        rect.center(),
                        Align2::CENTER_CENTER,
                        if self.active {
                            "正在读取目录"
                        } else {
                            "此目录没有已分配空间"
                        },
                        14.0,
                        SUB,
                    );
                    return;
                }
                let mut clicked = None;
                if ring {
                    let radius = (rect.width().min(rect.height()) / 2.0 - 4.0).max(20.0);
                    let total = self.current_node.as_ref().map(|n| n.bytes).unwrap_or(0);
                    let pointer = ui.ctx().pointer_hover_pos();
                    let response = ui.interact(rect, ui.id().with("rings"), Sense::click());
                    Self::rings(
                        ui,
                        &self.chart,
                        rect.center(),
                        radius,
                        0,
                        -2.94,
                        std::f32::consts::TAU - 2.94,
                        total,
                        pointer,
                        response.clicked(),
                        &mut clicked,
                        &mut self.hover,
                    );
                    text(
                        ui,
                        rect.center() - vec2(0.0, 21.0),
                        Align2::CENTER_CENTER,
                        if self.active {
                            "已扫描"
                        } else {
                            "文件占用"
                        },
                        12.0,
                        SUB,
                    );
                    let label = size(total);
                    let (value, unit) = label.split_once(' ').unwrap_or((&label, ""));
                    text(
                        ui,
                        rect.center() + vec2(0.0, 5.0),
                        Align2::CENTER_CENTER,
                        value,
                        (radius * 0.23).clamp(22.0, 37.0),
                        INK,
                    );
                    text(
                        ui,
                        rect.center() + vec2(0.0, 31.0),
                        Align2::CENTER_CENTER,
                        unit,
                        12.0,
                        SUB,
                    );
                    if let Some(id) = self.hover
                        && let Some(branch) = find_branch(&self.chart, id)
                    {
                        response.on_hover_text(format!(
                            "{}\n{}",
                            branch.node.name,
                            size(branch.node.bytes)
                        ));
                    }
                } else {
                    let mut tiles = vec![];
                    Self::tile_layout(&self.chart, rect, &mut tiles);
                    for (node, tile, color) in tiles {
                        let tile = tile.shrink(2.5);
                        let response = ui.interact(tile, ui.id().with(node.id), Sense::click());
                        let fill = color.lerp_to_gamma(PAPER, 0.32);
                        ui.painter().rect_filled(
                            tile,
                            7,
                            if response.hovered() {
                                color.lerp_to_gamma(PAPER, 0.18)
                            } else {
                                fill
                            },
                        );
                        if self.selected.as_ref().is_some_and(|n| n.id == node.id) {
                            ui.painter().rect_stroke(
                                tile,
                                7,
                                Stroke::new(2.0_f32, ACCENT),
                                StrokeKind::Inside,
                            );
                        }
                        if tile.width() > 90.0 && tile.height() > 97.0 {
                            folder_icon(
                                ui,
                                Rect::from_min_size(tile.min + vec2(9.0, 8.0), vec2(22.0, 22.0)),
                                INK,
                                node.directory,
                            );
                        }
                        if tile.width() > 62.0 && tile.height() > 39.0 {
                            let clip = tile.shrink(9.0);
                            let painter = ui.painter().with_clip_rect(clip);
                            let font = if tile.width() > 150.0 { 15.0 } else { 12.0 };
                            painter.text(
                                pos2(clip.left(), clip.bottom() - 17.0),
                                Align2::LEFT_BOTTOM,
                                &node.name,
                                FontId::proportional(font),
                                INK,
                            );
                            painter.text(
                                clip.left_bottom(),
                                Align2::LEFT_BOTTOM,
                                size(node.bytes),
                                FontId::proportional(12.0),
                                INK,
                            );
                        }
                        if response.hovered() {
                            self.hover = Some(node.id);
                        }
                        if response.clicked() {
                            clicked = Some(node.clone());
                        }
                        response.on_hover_text(format!("{}\n{}", node.name, size(node.bytes)));
                    }
                }
                if let Some(node) = clicked {
                    self.navigate(&node);
                }
            });
    }
    fn tile_layout(branches: &[Branch], rect: Rect, out: &mut Vec<(Node, Rect, Color32)>) {
        let branches = branches
            .iter()
            .filter(|b| b.node.bytes > 0)
            .collect::<Vec<_>>();
        fn split(nodes: &[&Branch], r: Rect, out: &mut Vec<(Node, Rect, Color32)>) {
            if nodes.is_empty() {
                return;
            }
            if nodes.len() == 1 {
                out.push((nodes[0].node.clone(), r, nodes[0].color));
                return;
            }
            let total = nodes.iter().map(|n| n.node.bytes as f64).sum::<f64>();
            let mut cut = 1;
            let mut sum = nodes[0].node.bytes as f64;
            while cut < nodes.len() - 1 {
                let next = sum + nodes[cut].node.bytes as f64;
                if (next - total / 2.0).abs() >= (sum - total / 2.0).abs() {
                    break;
                }
                sum = next;
                cut += 1;
            }
            let ratio = (sum / total) as f32;
            if r.width() >= r.height() {
                let x = r.left() + r.width() * ratio;
                split(
                    &nodes[..cut],
                    Rect::from_min_max(r.min, pos2(x, r.bottom())),
                    out,
                );
                split(
                    &nodes[cut..],
                    Rect::from_min_max(pos2(x, r.top()), r.max),
                    out,
                );
            } else {
                let y = r.top() + r.height() * ratio;
                split(
                    &nodes[..cut],
                    Rect::from_min_max(r.min, pos2(r.right(), y)),
                    out,
                );
                split(
                    &nodes[cut..],
                    Rect::from_min_max(pos2(r.left(), y), r.max),
                    out,
                );
            }
        }
        split(&branches, rect, out);
    }
    #[allow(clippy::too_many_arguments)]
    fn rings(
        ui: &Ui,
        nodes: &[Branch],
        center: Pos2,
        radius: f32,
        depth: usize,
        start: f32,
        end: f32,
        total: u64,
        pointer: Option<Pos2>,
        click: bool,
        selected: &mut Option<Node>,
        hover: &mut Option<i64>,
    ) {
        if total == 0 || depth > 2 {
            return;
        }
        let inner = radius * (0.40 + depth as f32 * 0.20);
        let outer = radius * (0.60 + depth as f32 * 0.20) - 2.0;
        let mut angle = start;
        for branch in nodes {
            if branch.node.bytes == 0 {
                continue;
            }
            let next = angle + (end - start) * (branch.node.bytes as f64 / total as f64) as f32;
            let gap = (1.3 / outer).min((next - angle) / 4.0);
            let a = angle + gap;
            let b = next - gap;
            let steps = (((b - a) * outer / 5.0).ceil() as usize).max(2);
            let mut mesh = Mesh::default();
            for i in 0..=steps {
                let v = Vec2::angled(a + (b - a) * i as f32 / steps as f32);
                mesh.colored_vertex(center + v * inner, branch.color);
                mesh.colored_vertex(center + v * outer, branch.color);
            }
            for i in 0..steps as u32 {
                let j = i * 2;
                mesh.add_triangle(j, j + 1, j + 2);
                mesh.add_triangle(j + 1, j + 3, j + 2);
            }
            ui.painter().add(Shape::mesh(mesh));
            if let Some(pointer) = pointer {
                let v = pointer - center;
                let mut theta = v.angle();
                while theta < start {
                    theta += std::f32::consts::TAU;
                }
                if v.length() >= inner && v.length() <= outer && theta >= a && theta <= b {
                    *hover = Some(branch.node.id);
                    if click {
                        *selected = Some(branch.node.clone());
                    }
                }
            }
            Self::rings(
                ui,
                &branch.children,
                center,
                radius,
                depth + 1,
                angle,
                next,
                branch.node.bytes,
                pointer,
                click,
                selected,
                hover,
            );
            angle = next;
        }
    }
    fn list(&mut self, ui: &mut Ui, height: f32) {
        ui.label(
            RichText::new(if self.large {
                "大文件"
            } else {
                "目录占用"
            })
            .size(16.0)
            .strong(),
        );
        ui.add_space(5.0);
        ui.separator();
        let mut clicked = None;
        ScrollArea::vertical()
            .id_salt("files")
            .max_height(height - 70.0)
            .auto_shrink([false, false])
            .show_rows(ui, 49.0, self.rows.len(), |ui, range| {
                for i in range {
                    let node = &self.rows[i];
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 49.0), Sense::click());
                    let color = self
                        .chart
                        .iter()
                        .find(|b| b.node.id == node.id)
                        .map(|b| b.color)
                        .unwrap_or(COLORS[i % 6]);
                    let highlight = response.hovered()
                        || self.hover == Some(node.id)
                        || self.selected.as_ref().is_some_and(|n| n.id == node.id);
                    if highlight {
                        ui.painter().rect_filled(rect, 7, MINT);
                    }
                    let icon = Rect::from_min_size(rect.min + vec2(2.0, 9.0), vec2(31.0, 31.0));
                    ui.painter()
                        .rect_filled(icon, 8, color.lerp_to_gamma(PAPER, 0.75));
                    folder_icon(ui, icon, color.gamma_multiply(0.78), node.directory);
                    let text_rect = Rect::from_min_max(
                        rect.min + vec2(44.0, 5.0),
                        pos2(rect.right() - 105.0, rect.bottom() - 4.0),
                    );
                    ui.painter().with_clip_rect(text_rect).text(
                        text_rect.left_center(),
                        Align2::LEFT_CENTER,
                        &node.name,
                        FontId::proportional(14.0),
                        INK,
                    );
                    text(
                        ui,
                        pos2(rect.right() - 17.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        size(node.bytes),
                        13.0,
                        INK,
                    );
                    text(
                        ui,
                        pos2(rect.right() - 3.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        if node.directory { "›" } else { "·" },
                        15.0,
                        SUB,
                    );
                    ui.painter()
                        .hline(rect.x_range(), rect.bottom(), Stroke::new(0.6_f32, RULE));
                    if response.clicked() {
                        clicked = Some(node.clone());
                    }
                    let detail = if node.directory {
                        format!(
                            "{}\n{} 个文件 · {} 个子目录\n{}",
                            node.name,
                            node.files,
                            node.folders,
                            match node.status {
                                2 => "部分内容读取失败",
                                3 => "链接目录，跳过遍历",
                                _ => "按实际占用排序",
                            }
                        )
                    } else {
                        format!(
                            "{}\n实际占用 {} · 文件大小 {}",
                            node.name,
                            size(node.bytes),
                            size(node.logical)
                        )
                    };
                    response.on_hover_text(detail);
                }
            });
        if let Some(node) = clicked {
            self.navigate(&node);
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(self.page > 0, Button::new("‹")).clicked() {
                self.page -= 1;
                self.refresh();
            }
            ui.label(
                RichText::new(format!("第 {} 页", self.page + 1))
                    .size(12.0)
                    .color(SUB),
            );
            if ui
                .add_enabled(self.rows.len() == scan::PAGE, Button::new("›"))
                .clicked()
            {
                self.page += 1;
                self.refresh();
            }
        });
    }
    fn footer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("●").color(COLORS[0]).size(10.0));
            let state = if self.paused {
                "已暂停"
            } else if self.active {
                "正在扫描"
            } else if self.stats.stopped {
                "已停止"
            } else if self.stats.errors > 0 {
                "扫描结束 · 部分目录未读取"
            } else if self.stats.complete {
                "扫描完成"
            } else {
                "就绪"
            };
            ui.label(RichText::new(state).color(SUB).size(12.0));
            if self.job.is_some() {
                ui.label(
                    RichText::new(format!(
                        "{} 个文件 · {}",
                        self.stats.files,
                        size(self.stats.bytes)
                    ))
                    .size(12.0)
                    .color(SUB),
                );
            }
            if self.active {
                if ui
                    .button(if self.paused { "继续" } else { "暂停" })
                    .clicked()
                {
                    self.paused = !self.paused;
                    if let Some(job) = &self.job {
                        job.control.pause(self.paused);
                    }
                }
                if ui.button("停止").clicked() {
                    if let Some(job) = &self.job {
                        job.control.stop();
                    }
                    self.paused = false;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button(self.settings.profile.label(), |ui| {
                    for profile in [Profile::Gentle, Profile::Balanced] {
                        if ui
                            .selectable_label(profile == self.settings.profile, profile.label())
                            .clicked()
                        {
                            self.settings.profile = profile;
                            if let Some(job) = &self.job {
                                job.control.profile(profile);
                            }
                            self.save_preferences();
                            ui.close();
                        }
                    }
                });
                if self.stats.errors + self.stats.skipped_links + self.stats.estimated > 0
                    && ui
                        .button(format!(
                            "跳过 / 异常 {}",
                            self.stats.errors + self.stats.skipped_links + self.stats.estimated
                        ))
                        .clicked()
                {
                    self.issue_rows = self
                        .index
                        .as_ref()
                        .and_then(|i| i.issues().ok())
                        .unwrap_or_default();
                    self.issues_open = true;
                }
            });
        });
    }
    fn dialogs(&mut self, ctx: &Context) {
        if self.detail_open
            && let Some(node) = self.selected.clone()
        {
            let mut open = true;
            let mut locate = false;
            let mut parent = false;
            Window::new("文件详情")
                .id(Id::new("detail"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(370.0)
                .show(ctx, |ui| {
                    ui.label(RichText::new(&node.name).size(18.0).strong());
                    let path = self.index.as_ref().and_then(|i| i.path(node.id).ok());
                    if let Some(path) = &path {
                        ui.label(RichText::new(display_path(path)).size(12.0).color(SUB));
                    }
                    ui.add_space(12.0);
                    ui.label(format!("实际占用    {}", size(node.bytes)));
                    ui.label(format!("文件大小    {}", size(node.logical)));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        locate = ui.button("在资源管理器中定位").clicked();
                        parent = ui.button("查看所在目录").clicked();
                    });
                    if locate && let Some(path) = path {
                        if let Err(error) = std::process::Command::new("explorer.exe")
                            .arg(format!("/select,{}", path.display()))
                            .spawn()
                        {
                            self.notice = Some(format!("无法打开资源管理器。{error}"));
                        }
                    }
                });
            if parent {
                if let Some(id) = node.parent {
                    self.current = id;
                    self.large = false;
                    self.page = 0;
                    self.refresh();
                }
                open = false;
            }
            if !open {
                self.detail_open = false;
            }
        }
        if let Some(message) = self.notice.clone() {
            let mut open = true;
            Window::new("提示")
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(message);
                });
            if !open {
                self.notice = None;
            }
        }
        Window::new("扫描详情")
            .open(&mut self.issues_open)
            .default_width(600.0)
            .default_height(350.0)
            .show(ctx, |ui| {
                ui.label(format!(
                    "读取失败 {} · 跳过目录链接 {} · 估算占用 {} · 已去重硬链接 {}",
                    self.stats.errors,
                    self.stats.skipped_links,
                    self.stats.estimated,
                    self.stats.hardlinks
                ));
                ui.label(
                    RichText::new("目录链接保留在列表中，遍历时跳过。")
                        .size(12.0)
                        .color(SUB),
                );
                ScrollArea::vertical().show(ui, |ui| {
                    for (path, error) in &self.issue_rows {
                        ui.label(path);
                        ui.label(RichText::new(error).size(12.0).color(SUB));
                        ui.separator();
                    }
                });
            });
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        TopBottomPanel::top("top")
            .frame(
                Frame::new()
                    .fill(PAPER)
                    .inner_margin(Margin::symmetric(24, 12)),
            )
            .show(ctx, |ui| self.heading(ui));
        TopBottomPanel::bottom("status")
            .frame(
                Frame::new()
                    .fill(PAPER)
                    .inner_margin(Margin::symmetric(22, 8)),
            )
            .show(ctx, |ui| self.footer(ui));
        CentralPanel::default()
            .frame(Frame::new().fill(PAPER).inner_margin(26))
            .show(ctx, |ui| {
                if self.root_path.is_none() {
                    self.empty(ui);
                    return;
                }
                self.disk_header(ui);
                self.toolbar(ui);
                self.hover = None;
                let height = ui.available_height().max(280.0);
                if self.large {
                    self.list(ui, height);
                } else if self.settings.mode == Mode::Both {
                    ui.columns(2, |columns| {
                        self.map(&mut columns[0], true, height - 65.0);
                        self.map(&mut columns[1], false, height - 65.0);
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        for branch in self.chart.clone().into_iter().take(12) {
                            if ui
                                .add(
                                    Button::new(
                                        RichText::new(format!(
                                            "● {}  {}",
                                            branch.node.name,
                                            size(branch.node.bytes)
                                        ))
                                        .size(12.0)
                                        .color(branch.color.gamma_multiply(0.7)),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                self.navigate(&branch.node);
                            }
                        }
                    });
                } else {
                    ui.columns(2, |columns| {
                        self.map(&mut columns[0], self.settings.mode == Mode::Ring, height);
                        self.list(&mut columns[1], height);
                    });
                }
            });
        self.dialogs(ctx);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_preferences();
        if let Some(job) = &self.job {
            job.control.stop();
        }
    }
}
fn find_branch(nodes: &[Branch], id: i64) -> Option<&Branch> {
    for branch in nodes {
        if branch.node.id == id {
            return Some(branch);
        }
        if let Some(found) = find_branch(&branch.children, id) {
            return Some(found);
        }
    }
    None
}
fn display_path(path: &std::path::Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else {
        value.strip_prefix("\\\\?\\").unwrap_or(&value).to_owned()
    }
}
