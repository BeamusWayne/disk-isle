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
const AMBER: Color32 = Color32::from_rgb(146, 115, 68);
const AMBER_FILL: Color32 = Color32::from_rgb(242, 234, 220);
const COMPLETION_DURATION: Duration = Duration::from_secs(6);
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
struct CompletionNotice {
    expires_at: Instant,
    interacting: bool,
}
impl CompletionNotice {
    fn new(now: Instant) -> Self {
        Self {
            expires_at: now + COMPLETION_DURATION,
            interacting: false,
        }
    }

    fn update(&mut self, now: Instant, interacting: bool) -> bool {
        if interacting || self.interacting {
            self.expires_at = now + COMPLETION_DURATION;
        }
        self.interacting = interacting;
        now < self.expires_at
    }
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
    completion: Option<CompletionNotice>,
    details_button_id: Option<Id>,
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
#[derive(Clone, Copy)]
enum Icon {
    Folder,
    Refresh,
    Up,
    Down,
    Check,
    Details,
    Alert,
    Pause,
    Play,
    Stop,
    Leaf,
}
#[derive(Clone, Copy)]
enum ButtonTone {
    Ghost,
    Outline,
    Tonal,
    Warning,
}

fn draw_icon(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    let center = rect.center();
    let painter = ui.painter();
    let stroke = Stroke::new(1.4_f32, color);
    let line = |a: Vec2, b: Vec2| {
        painter.line_segment([center + a, center + b], stroke);
    };
    match icon {
        Icon::Folder => folder_icon(ui, rect, color, true),
        Icon::Refresh => {
            let points = (0..=20)
                .map(|i| {
                    let angle = -0.65 + (std::f32::consts::TAU - 0.9) * i as f32 / 20.0;
                    center + Vec2::angled(angle) * 6.0
                })
                .collect();
            painter.add(Shape::line(points, stroke));
            line(vec2(5.0, -7.0), vec2(5.0, -2.0));
            line(vec2(5.0, -2.0), vec2(0.0, -2.0));
        }
        Icon::Up => {
            line(vec2(-6.0, 0.0), vec2(6.0, 0.0));
            line(vec2(-6.0, 0.0), vec2(-1.0, -5.0));
            line(vec2(-6.0, 0.0), vec2(-1.0, 5.0));
        }
        Icon::Down => {
            line(vec2(-4.0, -2.0), vec2(0.0, 2.0));
            line(vec2(0.0, 2.0), vec2(4.0, -2.0));
        }
        Icon::Check => {
            line(vec2(-5.0, 0.0), vec2(-1.0, 4.0));
            line(vec2(-1.0, 4.0), vec2(6.0, -4.0));
        }
        Icon::Details => {
            for (x, h) in [(-5.0, 4.0), (0.0, 10.0), (5.0, 7.0)] {
                line(vec2(x, 6.0), vec2(x, 6.0 - h));
            }
        }
        Icon::Alert => {
            painter.circle_stroke(center, 7.0, stroke);
            line(vec2(0.0, -4.0), vec2(0.0, 1.0));
            painter.circle_filled(center + vec2(0.0, 4.0), 0.8, color);
        }
        Icon::Pause => {
            line(vec2(-3.0, -5.0), vec2(-3.0, 5.0));
            line(vec2(3.0, -5.0), vec2(3.0, 5.0));
        }
        Icon::Play => {
            painter.add(Shape::closed_line(
                vec![
                    center + vec2(-4.0, -5.0),
                    center + vec2(5.0, 0.0),
                    center + vec2(-4.0, 5.0),
                ],
                stroke,
            ));
        }
        Icon::Stop => {
            painter.rect_stroke(
                Rect::from_center_size(center, vec2(10.0, 10.0)),
                1,
                stroke,
                StrokeKind::Inside,
            );
        }
        Icon::Leaf => {
            painter.add(Shape::closed_line(
                vec![
                    center + vec2(-5.0, 5.0),
                    center + vec2(-6.0, -1.0),
                    center + vec2(-1.0, -5.0),
                    center + vec2(6.0, -6.0),
                    center + vec2(5.0, 1.0),
                    center + vec2(1.0, 5.0),
                ],
                stroke,
            ));
            line(vec2(-6.0, 6.0), vec2(2.0, -2.0));
        }
    }
}

fn action_button(ui: &mut Ui, label: &str, icon: Icon, tone: ButtonTone) -> Response {
    let enabled = ui.is_enabled();
    let mut color = match tone {
        ButtonTone::Tonal => ACCENT,
        ButtonTone::Warning => AMBER,
        _ => SUB,
    };
    if !enabled {
        color = color.lerp_to_gamma(PAPER, 0.52);
    }
    let font = FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let width = galley.size().x + 46.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let mut fill = match tone {
            ButtonTone::Tonal => MINT,
            ButtonTone::Warning => AMBER_FILL,
            ButtonTone::Outline => PAPER,
            ButtonTone::Ghost if response.hovered() || response.has_focus() => PANEL,
            _ => Color32::TRANSPARENT,
        };
        if enabled && response.is_pointer_button_down_on() {
            fill = fill.lerp_to_gamma(ACCENT, 0.12);
        } else if enabled
            && response.hovered()
            && matches!(tone, ButtonTone::Tonal | ButtonTone::Warning)
        {
            fill = fill.lerp_to_gamma(ACCENT, 0.06);
        } else if response.hovered() && matches!(tone, ButtonTone::Outline) {
            fill = PANEL;
        }
        ui.painter().rect_filled(rect, 8, fill);
        if matches!(tone, ButtonTone::Outline) || response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                8,
                Stroke::new(1.0_f32, if response.has_focus() { ACCENT } else { RULE }),
                StrokeKind::Inside,
            );
        }
        draw_icon(
            ui,
            Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(17.0, 17.0)),
            icon,
            color,
        );
        ui.painter().galley(
            pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}
fn mode_button(ui: &mut Ui, mode: Mode, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(70.0, 30.0), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            ui.is_enabled(),
            active,
            match mode {
                Mode::Grid => "网格",
                Mode::Ring => "环形",
                Mode::Both => "对比",
            },
        )
    });
    if active || response.hovered() {
        ui.painter()
            .rect_filled(rect, 7, if active { MINT } else { PANEL });
    }
    if response.has_focus() {
        ui.painter()
            .rect_stroke(rect, 7, Stroke::new(1.0_f32, ACCENT), StrokeKind::Inside);
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
    pub fn new(ctx: &Context, initial: Option<PathBuf>) -> Self {
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
        ctx.set_fonts(fonts);
        let mut style = (*ctx.style()).clone();
        style.visuals = Visuals::light();
        style.visuals.override_text_color = Some(INK);
        style.visuals.panel_fill = PAPER;
        style.visuals.window_fill = PAPER;
        style.visuals.selection.bg_fill = MINT;
        style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.inactive.bg_fill = PANEL;
        style.visuals.widgets.inactive.weak_bg_fill = PANEL;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, RULE);
        style.visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
        style.visuals.widgets.hovered.bg_fill = MINT;
        style.visuals.widgets.hovered.weak_bg_fill = MINT;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, RULE);
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
        style.visuals.widgets.active.bg_fill = MINT;
        style.visuals.widgets.active.weak_bg_fill = MINT;
        style.visuals.widgets.active.corner_radius = CornerRadius::same(8);
        style.visuals.menu_corner_radius = CornerRadius::same(12);
        style.visuals.window_stroke = Stroke::new(1.0_f32, RULE);
        style.spacing.item_spacing = vec2(9.0, 8.0);
        style.spacing.button_padding = vec2(10.0, 7.0);
        ctx.set_style(style);
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
            completion: None,
            details_button_id: None,
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
        self.completion = None;
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
                self.completion = None;
                self.issues_open = false;
                self.issue_rows.clear();
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
            self.handle_event(event);
        }
        if self.active {
            if self.last_refresh.elapsed() >= Duration::from_millis(650) {
                self.refresh();
            }
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Progress(stats) => self.stats = stats,
            Event::Finished(stats, error) => {
                self.completion = if error.is_none() && stats.complete && !stats.stopped {
                    Some(CompletionNotice::new(Instant::now()))
                } else {
                    None
                };
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
    fn go_up(&mut self) {
        if let Some(parent) = self.current_node.as_ref().and_then(|node| node.parent) {
            self.current = parent;
            self.page = 0;
            self.large = false;
            self.refresh();
        }
    }
    fn open_scan_details(&mut self) {
        self.issue_rows = self
            .index
            .as_ref()
            .and_then(|index| index.issues().ok())
            .unwrap_or_default();
        self.issues_open = true;
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
                    .add_enabled_ui(!self.active, |ui| {
                        action_button(ui, "选择文件夹", Icon::Folder, ButtonTone::Ghost)
                    })
                    .inner
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
                .unwrap_or_else(|| format!("本地磁盘 ({})", path.trim_end_matches(['\\', '/'])));
            let title_width = (ui.available_width() - 370.0).max(190.0);
            ui.allocate_ui_with_layout(
                vec2(title_width, 58.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    let response = ui
                        .add_enabled(
                            !self.active,
                            Button::new(RichText::new(format!("{name}    ")).size(25.0).strong())
                                .frame(false)
                                .corner_radius(9)
                                .truncate()
                                .min_size(vec2(0.0, 36.0)),
                        )
                        .on_hover_text("选择磁盘");
                    draw_icon(
                        ui,
                        Rect::from_center_size(
                            response.rect.right_center() - vec2(12.0, 0.0),
                            vec2(16.0, 16.0),
                        ),
                        Icon::Down,
                        SUB,
                    );
                    let mut selected_path = None;
                    Popup::menu(&response).width(260.0).show(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for drive in platform::drives() {
                            let selected = self.root_path.as_ref() == Some(&drive);
                            let capacity = platform::space(&drive)
                                .map(|(total, used)| format!("{} / {}", size(used), size(total)))
                                .unwrap_or_else(|| "选择并扫描".into());
                            let (rect, item) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 58.0),
                                Sense::click(),
                            );
                            item.widget_info(|| {
                                WidgetInfo::labeled(
                                    WidgetType::Button,
                                    true,
                                    format!("本地磁盘 {}", drive.display()),
                                )
                            });
                            if selected || item.hovered() || item.has_focus() {
                                ui.painter().rect_filled(
                                    rect,
                                    8,
                                    if selected { MINT } else { PANEL },
                                );
                            }
                            folder_icon(
                                ui,
                                Rect::from_min_size(rect.min + vec2(11.0, 18.0), vec2(22.0, 22.0)),
                                SUB,
                                true,
                            );
                            text(
                                ui,
                                rect.min + vec2(46.0, 11.0),
                                Align2::LEFT_TOP,
                                format!(
                                    "本地磁盘 ({})",
                                    drive.display().to_string().trim_end_matches('\\')
                                ),
                                14.0,
                                INK,
                            );
                            text(
                                ui,
                                rect.min + vec2(46.0, 33.0),
                                Align2::LEFT_TOP,
                                capacity,
                                12.0,
                                SUB,
                            );
                            if selected {
                                draw_icon(
                                    ui,
                                    Rect::from_center_size(
                                        rect.right_center() - vec2(17.0, 0.0),
                                        vec2(16.0, 16.0),
                                    ),
                                    Icon::Check,
                                    ACCENT,
                                );
                            }
                            if item.clicked() {
                                selected_path = Some(drive);
                                ui.close();
                            }
                        }
                    });
                    if let Some(path) = selected_path {
                        self.begin(path);
                    }
                    ui.add(Label::new(RichText::new(&path).size(12.0).color(SUB)).truncate())
                        .on_hover_text(&path);
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled_ui(!self.active, |ui| {
                        action_button(ui, "重新扫描", Icon::Refresh, ButtonTone::Tonal)
                    })
                    .inner
                    .clicked()
                    && let Some(path) = self.root_path.clone()
                {
                    self.begin(path);
                }
                if let Some((total, used)) = self.space {
                    ui.add_space(12.0);
                    ui.allocate_ui_with_layout(
                        vec2(190.0, 36.0),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("已用 {}", size(used)))
                                        .size(12.0)
                                        .color(SUB),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(format!("共 {}", size(total)))
                                            .size(12.0)
                                            .color(SUB),
                                    );
                                });
                            });
                            let (rect, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 5.0),
                                Sense::hover(),
                            );
                            ui.painter().rect_filled(rect, 3, RULE);
                            let used_rect = Rect::from_min_size(
                                rect.min,
                                vec2(
                                    rect.width()
                                        * (used as f64 / total.max(1) as f64).clamp(0.0, 1.0)
                                            as f32,
                                    5.0,
                                ),
                            );
                            ui.painter().rect_filled(used_rect, 3, COLORS[0]);
                        },
                    );
                }
            });
        });
        ui.add_space(17.0);
        ui.separator();
        ui.add_space(6.0);
    }
    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let has_parent = self.current_node.as_ref().and_then(|n| n.parent).is_some();
            if ui
                .add_enabled_ui(has_parent, |ui| {
                    action_button(ui, "上一级", Icon::Up, ButtonTone::Outline)
                })
                .inner
                .on_disabled_hover_text("已在扫描根目录")
                .on_hover_text("返回上一层 · Alt + ←")
                .clicked()
            {
                self.go_up();
            }
            let path_width = (ui.available_width() - 230.0).max(40.0);
            ui.allocate_ui_with_layout(
                vec2(path_width, 36.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    let breadcrumbs = self.breadcrumbs.clone();
                    for (position, node) in breadcrumbs.iter().enumerate() {
                        if position > 0 && position + 2 < breadcrumbs.len() {
                            if position == 1 {
                                ui.label(RichText::new("…").color(SUB));
                            }
                            continue;
                        }
                        let name = if node.id == 1 {
                            "根目录"
                        } else {
                            &node.name
                        };
                        let width = (path_width / 3.5).clamp(35.0, 125.0);
                        if ui
                            .add_sized(
                                [width, 30.0],
                                Button::new(RichText::new(name).size(12.0).color(SUB))
                                    .frame(false)
                                    .truncate(),
                            )
                            .on_hover_text(&node.name)
                            .clicked()
                        {
                            self.navigate(node);
                        }
                        if position + 1 < breadcrumbs.len() {
                            ui.label(RichText::new("›").color(SUB));
                        }
                    }
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                Frame::new()
                    .fill(PANEL)
                    .corner_radius(10)
                    .inner_margin(3)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.horizontal(|ui| {
                            for mode in [Mode::Both, Mode::Ring, Mode::Grid] {
                                if mode_button(ui, mode, self.settings.mode == mode).clicked() {
                                    self.settings.mode = mode;
                                    self.save_preferences();
                                }
                            }
                        });
                    });
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
            let (status_rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            if self.stats.complete {
                ui.painter()
                    .circle_stroke(status_rect.center(), 7.0, Stroke::new(1.2_f32, ACCENT));
                draw_icon(ui, status_rect.shrink(2.0), Icon::Check, ACCENT);
            } else {
                ui.painter()
                    .circle_filled(status_rect.center(), 3.0, COLORS[0]);
            }
            let state = if self.paused {
                "已暂停"
            } else if self.active {
                "正在扫描"
            } else if self.stats.stopped {
                "已停止"
            } else if self.stats.complete {
                "扫描完成"
            } else if self.stats.errors > 0 {
                "扫描中断"
            } else {
                "就绪"
            };
            ui.label(RichText::new(state).color(SUB).size(12.0));
            if self.job.is_some() || self.stats.complete {
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
                if action_button(
                    ui,
                    if self.paused { "继续" } else { "暂停" },
                    if self.paused { Icon::Play } else { Icon::Pause },
                    ButtonTone::Outline,
                )
                .clicked()
                {
                    self.paused = !self.paused;
                    if let Some(job) = &self.job {
                        job.control.pause(self.paused);
                    }
                }
                if action_button(ui, "停止", Icon::Stop, ButtonTone::Outline).clicked() {
                    if let Some(job) = &self.job {
                        job.control.stop();
                    }
                    self.paused = false;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let profile_button = action_button(
                    ui,
                    self.settings.profile.label(),
                    Icon::Leaf,
                    ButtonTone::Ghost,
                );
                Popup::menu(&profile_button).show(|ui| {
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
                if self.job.is_some() || self.stats.complete {
                    let has_errors = self.stats.errors > 0;
                    let label = if has_errors {
                        format!("{} 项未读取", self.stats.errors)
                    } else {
                        "扫描详情".into()
                    };
                    let response = action_button(
                        ui,
                        &label,
                        if has_errors {
                            Icon::Alert
                        } else {
                            Icon::Details
                        },
                        if has_errors {
                            ButtonTone::Warning
                        } else {
                            ButtonTone::Ghost
                        },
                    );
                    self.details_button_id = Some(response.id);
                    if response.clicked() {
                        self.open_scan_details();
                    }
                }
            });
        });
    }
    fn completion_feedback(&mut self, ctx: &Context) {
        if self.completion.is_none() {
            return;
        }
        let has_errors = self.stats.errors > 0;
        let color = if has_errors { AMBER } else { ACCENT };
        let tint = if has_errors { AMBER_FILL } else { MINT };
        let title = if has_errors {
            format!("扫描完成，{} 项未读取", self.stats.errors)
        } else {
            "扫描完成".into()
        };
        let mut details = false;
        let mut close = false;
        let mut focused = false;
        let mut close_had_focus = false;
        let area = Area::new(Id::new("completion_notice"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-26.0, -62.0))
            .movable(false)
            .order(Order::Foreground)
            .sense(Sense::hover())
            .show(ctx, |ui| {
                Frame::new()
                    .fill(PAPER)
                    .stroke(Stroke::new(1.0_f32, RULE))
                    .corner_radius(13)
                    .inner_margin(Margin::symmetric(14, 12))
                    .shadow(epaint::Shadow {
                        offset: [0, 4],
                        blur: 18,
                        spread: 0,
                        color: Color32::from_black_alpha(18),
                    })
                    .show(ui, |ui| {
                        ui.set_width(360.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(32.0, 40.0), Sense::hover());
                            ui.painter().circle_filled(rect.center(), 15.0, tint);
                            draw_icon(
                                ui,
                                Rect::from_center_size(rect.center(), vec2(17.0, 17.0)),
                                if has_errors { Icon::Alert } else { Icon::Check },
                                color,
                            );
                            ui.allocate_ui_with_layout(
                                vec2(190.0, 40.0),
                                Layout::top_down(Align::LEFT),
                                |ui| {
                                    ui.spacing_mut().item_spacing.y = 3.0;
                                    ui.add(
                                        Label::new(RichText::new(&title).size(14.0).strong())
                                            .truncate(),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} 个文件 · {}",
                                            self.stats.files,
                                            size(self.stats.bytes)
                                        ))
                                        .size(12.0)
                                        .color(SUB),
                                    );
                                },
                            );
                            let details_response = ui.add(
                                Button::new(RichText::new("详情 ↗").size(12.0).color(ACCENT))
                                    .frame(false)
                                    .min_size(vec2(43.0, 32.0)),
                            );
                            details = details_response.clicked();
                            focused |= details_response.has_focus();
                            let close_response = ui
                                .add(
                                    Button::new(RichText::new("×").size(19.0).color(SUB))
                                        .frame(false)
                                        .min_size(vec2(25.0, 30.0)),
                                )
                                .on_hover_text("收起完成提示");
                            close_response.widget_info(|| {
                                WidgetInfo::labeled(WidgetType::Button, true, "收起完成提示")
                            });
                            close = close_response.clicked();
                            close_had_focus = close_response.has_focus();
                            focused |= close_had_focus;
                        });
                    });
            });
        let now = Instant::now();
        let interacting = area.response.contains_pointer() || focused;
        if let Some(completion) = &mut self.completion {
            if completion.update(now, interacting) {
                ctx.request_repaint_after(completion.expires_at.saturating_duration_since(now));
            } else {
                self.completion = None;
                ctx.request_repaint();
            }
        }
        if details {
            self.open_scan_details();
            self.completion = None;
        } else if close {
            self.completion = None;
            if close_had_focus && let Some(id) = self.details_button_id {
                ctx.memory_mut(|memory| memory.request_focus(id));
            }
        }
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
                ui.label(
                    RichText::new(if self.stats.stopped {
                        "已停止"
                    } else if self.stats.complete {
                        "扫描完成"
                    } else if self.active {
                        "正在扫描"
                    } else {
                        "扫描中断"
                    })
                    .size(16.0)
                    .strong(),
                );
                ui.add_space(8.0);
                Grid::new("scan_summary")
                    .num_columns(2)
                    .spacing(vec2(24.0, 8.0))
                    .show(ui, |ui| {
                        for (label, value) in [
                            ("文件", self.stats.files.to_string()),
                            ("目录", self.stats.folders.to_string()),
                            ("实际占用", size(self.stats.bytes)),
                            ("扫描用时", format!("{:.1} 秒", self.stats.seconds)),
                        ] {
                            ui.label(RichText::new(label).color(SUB));
                            ui.label(value);
                            ui.end_row();
                        }
                    });
                ui.add_space(16.0);
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
    fn show(&mut self, ctx: &Context) {
        self.poll(ctx);
        if !self.detail_open
            && !self.issues_open
            && self.notice.is_none()
            && ctx.input_mut(|input| input.consume_key(Modifiers::ALT, Key::ArrowLeft))
        {
            self.go_up();
        }
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
        self.completion_feedback(ctx);
        self.dialogs(ctx);
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
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

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Event;

    #[test]
    fn completion_expires_after_six_seconds() {
        let now = Instant::now();
        let mut notice = CompletionNotice::new(now);
        assert!(notice.update(now + Duration::from_millis(5999), false));
        assert!(!notice.update(now + COMPLETION_DURATION, false));
    }

    #[test]
    fn completion_keeps_six_seconds_after_interaction_ends() {
        let now = Instant::now();
        let mut notice = CompletionNotice::new(now);
        let later = now + Duration::from_secs(60);
        assert!(notice.update(later, true));
        assert!(notice.update(later + Duration::from_secs(30), true));
        let leave = later + Duration::from_secs(31);
        assert!(notice.update(leave, false));
        assert!(notice.update(leave + Duration::from_millis(5999), false));
        assert!(!notice.update(leave + COMPLETION_DURATION, false));
    }

    fn render(ctx: &Context, app: &mut App, events: Vec<Event>) -> FullOutput {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 760.0))),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| app.show(ctx))
    }

    fn label_position(output: &FullOutput, label: &str) -> Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let Shape::Text(text) = &shape.shape
                    && text.galley.text() == label
                {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Missing label {label}"))
    }

    fn click_label(ctx: &Context, app: &mut App, label: &str) {
        let output = render(ctx, app, vec![]);
        let position = label_position(&output, label);
        let _ = render(
            ctx,
            app,
            vec![
                Event::PointerMoved(position),
                Event::PointerButton {
                    pos: position,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        let _ = render(
            ctx,
            app,
            vec![Event::PointerButton {
                pos: position,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
    }

    #[test]
    fn completion_and_parent_navigation_work_through_real_ui() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("work")
            .join(format!("ui-check-{}", std::process::id()));
        let child = root.join("子目录");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("文件.txt"), vec![7u8; 4096]).unwrap();
        let ctx = Context::default();
        let mut app = App::new(&ctx, None);
        app.begin(root.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.active {
            app.poll(&ctx);
            assert!(Instant::now() < deadline, "scan did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(app.stats.complete);
        assert!(app.completion.is_some());
        let node = app
            .rows
            .iter()
            .find(|node| node.name == "子目录")
            .unwrap()
            .clone();
        app.navigate(&node);
        app.page = 3;
        app.large = true;
        click_label(&ctx, &mut app, "上一级");
        assert_eq!(app.current, 1);
        assert_eq!(app.page, 0);
        assert!(!app.large);
        assert_eq!(app.current_node.as_ref().unwrap().id, 1);
        click_label(&ctx, &mut app, "上一级");
        assert_eq!(app.current, 1);
        app.navigate(&node);
        let _ = render(
            &ctx,
            &mut app,
            vec![Event::Key {
                key: Key::ArrowLeft,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::ALT,
            }],
        );
        assert_eq!(app.current, 1);

        let focus_id = Id::new("completion-focus-test");
        let mut value = String::new();
        let mut frame = |ctx: &Context, app: &mut App| {
            ctx.run(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 760.0))),
                    ..Default::default()
                },
                |ctx| {
                    Area::new(Id::new("test-input"))
                        .fixed_pos(pos2(5.0, 5.0))
                        .show(ctx, |ui| {
                            ui.add(TextEdit::singleline(&mut value).id(focus_id));
                        });
                    app.show(ctx);
                },
            )
        };
        let _ = frame(&ctx, &mut app);
        ctx.memory_mut(|memory| memory.request_focus(focus_id));
        app.handle_event(scan::Event::Finished(
            Stats {
                complete: true,
                errors: 1,
                files: 1,
                bytes: 4096,
                ..Default::default()
            },
            None,
        ));
        let output = frame(&ctx, &mut app);
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(focus_id));
        label_position(&output, "扫描完成，1 项未读取");
        app.completion.as_mut().unwrap().expires_at = Instant::now() - Duration::from_secs(1);
        let _ = render(&ctx, &mut app, vec![Event::PointerGone]);
        assert!(app.completion.is_none());
        assert!(app.stats.complete);
        click_label(&ctx, &mut app, "1 项未读取");
        assert!(app.issues_open);
        app.issues_open = false;
        app.handle_event(scan::Event::Finished(
            Stats {
                stopped: true,
                ..Default::default()
            },
            None,
        ));
        assert!(app.completion.is_none());
        app.handle_event(scan::Event::Finished(
            Stats::default(),
            Some("read failed".into()),
        ));
        assert!(app.completion.is_none());
        app.notice = None;
        app.handle_event(scan::Event::Finished(
            Stats {
                complete: true,
                ..Default::default()
            },
            None,
        ));
        click_label(&ctx, &mut app, "重新扫描");
        assert!(app.completion.is_none());
        while app.active {
            app.poll(&ctx);
            assert!(Instant::now() < deadline, "rescan did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(app.completion.is_some());

        let index_path = app.job.as_ref().unwrap().path.clone();
        drop(app);
        std::fs::remove_dir_all(&root).unwrap();
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", index_path.display()));
        }
    }
}
