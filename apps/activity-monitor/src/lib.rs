//! Activity Monitor — what sevOS is doing right now: every process with its
//! share of the CPUs, its CPU time, threads, scheduler queue and the CPU it
//! last ran on, and underneath, a live load graph per CPU and the machine's
//! memory. Styled after macOS Activity Monitor's CPU tab.
//!
//! The figures come from the host through `ui::core::activity` (on NXU: the
//! scheduler's tick accounting, the process and thread tables and the
//! physical page allocator). The window samples once a second by default
//! (View > Update Frequency) from `App::tick`.
//!
//! Module map: `model` is the data (samples, deltas, sorting, number
//! formatting, host-tested); this file lays out and draws the window and
//! turns clicks, keys and the wheel into model changes.

#![cfg_attr(not(test), no_std)]

pub mod model;

use core::cell::UnsafeCell;

use ui::core::activity;
use ui::prelude::*;

use model::{Column, Filter, Model};

/// The window: 620 x 400 pt by default (a doubled 2x reference, see
/// `WindowConfig::effective_size`), resizable down to 460 x 320 pt.
const WINDOW_WIDTH: u32 = 1240;
const WINDOW_HEIGHT: u32 = 800;
const MIN_WINDOW_WIDTH: u32 = 920;
const MIN_WINDOW_HEIGHT: u32 = 640;

// Menu commands.
const COMMAND_FILTER_ALL: u32 = 1;
const COMMAND_FILTER_USER: u32 = 2;
const COMMAND_FILTER_SYSTEM: u32 = 3;
const COMMAND_INTERVAL_1: u32 = 11;
const COMMAND_INTERVAL_2: u32 = 12;
const COMMAND_INTERVAL_5: u32 = 13;
const COMMAND_SAMPLE_NOW: u32 = 20;

const TOOLBAR_BACKGROUND: Color = system_color::TITLEBAR;
const HEADER_BACKGROUND: Color = Color::rgb(248, 248, 250);
const STRIPE: Color = Color::rgb(245, 246, 248);
const DIVIDER: Color = system_color::SEPARATOR;
const HAIRLINE: Color = Color::rgb(226, 228, 233);
const PANEL_BACKGROUND: Color = Color::rgb(246, 247, 249);
const GRAPH_BACKGROUND: Color = Color::rgb(30, 32, 37);
const GRAPH_GRID: Color = Color::rgb(52, 55, 62);
const GRAPH_LOAD: Color = Color::rgb(52, 199, 89);
const GRAPH_LOAD_TOP: Color = Color::rgb(120, 230, 150);
const SEGMENT_TRACK: Color = Color::rgb(207, 209, 216);
const SEGMENT_PUCK: Color = Color::WHITE;
const SEGMENT_HOVER: Color = Color::rgb(196, 199, 207);
const HEADER_HOVER: Color = Color::rgb(236, 237, 241);

fn toolbar_height() -> u32 {
    scale::pt(44)
}
fn header_height() -> u32 {
    scale::pt(22)
}
fn row_height() -> u32 {
    scale::pt(20)
}
fn panel_height() -> u32 {
    scale::pt(104)
}
fn margin() -> i32 {
    scale::pt(12) as i32
}
fn cell_padding() -> i32 {
    scale::pt(8) as i32
}
fn label_size() -> u32 {
    scale::pt(12)
}
fn caption_size() -> u32 {
    scale::pt(11)
}

/// Fixed column widths in points; the name column takes what is left.
const fn column_points(column: Column) -> u32 {
    match column {
        Column::Name => 0,
        Column::Cpu => 58,
        Column::CpuTime => 76,
        Column::Threads => 60,
        Column::State => 70,
        Column::Queue => 52,
        Column::LastCpu => 42,
        Column::Pid => 50,
    }
}

/// Right-aligned (figures) or left-aligned (words).
const fn right_aligned(column: Column) -> bool {
    !matches!(column, Column::Name | Column::State)
}

struct StaticCell<T>(UnsafeCell<T>);
unsafe impl<T> Sync for StaticCell<T> {}

/// The model is ~15 KiB (processes, rows, history): a static, never a field
/// of the app, which the NXU desktop builds on a 16 KiB kernel stack.
static MODEL: StaticCell<Model> = StaticCell(UnsafeCell::new(Model::new()));

fn model() -> &'static mut Model {
    // SAFETY: one app instance, one thread (UIService's event loop).
    unsafe { &mut *MODEL.0.get() }
}

/// Where everything is, for a content area of one size: `draw` and `event`
/// both work from this, so they can never disagree.
#[derive(Clone, Copy)]
struct Layout {
    toolbar: Rect,
    segments: [Rect; 3],
    header: Rect,
    columns: [Rect; 8],
    rows: Rect,
    panel: Rect,
}

impl Layout {
    fn compute(size: Size) -> Self {
        let width = size.width;
        let toolbar = Rect::new(0, 0, width, toolbar_height());
        let panel_h = panel_height().min(size.height.saturating_sub(toolbar.size.height + header_height()));
        let panel = Rect::new(0, (size.height - panel_h) as i32, width, panel_h);
        let header = Rect::new(0, toolbar.size.height as i32, width, header_height());
        let rows_top = header.origin.y + header.size.height as i32;
        let rows = Rect::new(0, rows_top, width, (panel.origin.y - rows_top).max(0) as u32);

        // Columns: the fixed ones at their widths, the name gets the rest.
        // A narrow window drops the least telling columns first (a hidden
        // column is zero wide) rather than cut the name to nothing.
        let mut shown = [true; 8];
        let fixed_width = |shown: &[bool; 8]| -> u32 {
            Column::ALL.iter().zip(shown).filter(|(_, shown)| **shown).map(|(&column, _)| scale::pt(column_points(column))).sum()
        };
        for drop in [Column::Queue, Column::LastCpu, Column::State] {
            if width >= fixed_width(&shown) + scale::pt(150) {
                break;
            }
            shown[Column::ALL.iter().position(|&column| column == drop).unwrap_or(0)] = false;
        }
        let name_width = width.saturating_sub(fixed_width(&shown) + scale::pt(8)).max(scale::pt(90));
        let mut columns = [Rect::new(0, 0, 0, 0); 8];
        let mut x = 0i32;
        for (index, &column) in Column::ALL.iter().enumerate() {
            let w = if !shown[index] {
                0
            } else if column == Column::Name {
                name_width
            } else {
                scale::pt(column_points(column))
            };
            columns[index] = Rect::new(x, header.origin.y, w, header.size.height);
            x += w as i32;
        }

        // A three-way segmented control, right-aligned in the toolbar.
        let segment_w = scale::pt(58);
        let segment_h = scale::pt(24);
        let segments_x = width as i32 - margin() - (segment_w * 3) as i32;
        let segments_y = (toolbar.size.height as i32 - segment_h as i32) / 2;
        let segments = [0, 1, 2].map(|index| Rect::new(segments_x + (segment_w * index) as i32, segments_y, segment_w, segment_h));

        Self { toolbar, segments, header, columns, rows, panel }
    }

    fn visible_rows(&self) -> usize {
        (self.rows.size.height / row_height().max(1)) as usize
    }
}

const FILTERS: [Filter; 3] = [Filter::All, Filter::User, Filter::System];
const FILTER_SEGMENTS: [&str; 3] = ["All", "User", "System"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hover {
    Segment(usize),
    Header(usize),
}

pub struct ActivityMonitorApp {
    filter: Filter,
    sort: Column,
    descending: bool,
    /// The selected process, by uniqueid: it stays selected as rows re-sort.
    selected: Option<u64>,
    /// The first row shown (the table scrolls a whole row at a time).
    first_row: usize,
    hovered: Option<Hover>,
    interval_us: u64,
    next_sample_us: u64,
    last_size: Size,
}

impl ActivityMonitorApp {
    pub fn new() -> Self {
        let model = model();
        model.reset();
        model.sample();
        let app = Self {
            filter: Filter::All,
            sort: Column::Cpu,
            descending: true,
            selected: None,
            first_row: 0,
            hovered: None,
            interval_us: 1_000_000,
            // The second sample (the first with % CPU) comes quickly.
            next_sample_us: 0,
            last_size: Size::new(0, 0),
        };
        app.rebuild();
        app
    }

    fn rebuild(&self) {
        model().build_rows(self.filter, self.sort, self.descending);
    }

    fn set_filter(&mut self, filter: Filter) -> bool {
        if self.filter == filter {
            return false;
        }
        self.filter = filter;
        self.first_row = 0;
        self.rebuild();
        true
    }

    fn sort_by(&mut self, column: Column) {
        if self.sort == column {
            self.descending = !self.descending;
        } else {
            self.sort = column;
            self.descending = column.descending_first();
        }
        self.rebuild();
    }

    fn clamp_scroll(&mut self, layout: &Layout) {
        let max_first = model().row_count.saturating_sub(layout.visible_rows());
        self.first_row = self.first_row.min(max_first);
    }

    fn move_selection(&mut self, down: bool, layout: &Layout) -> bool {
        let model = model();
        if model.row_count == 0 {
            return false;
        }
        let current = self.selected.and_then(|uniqueid| model.row_of(uniqueid));
        let next = match current {
            None => 0,
            Some(row) if down => (row + 1).min(model.row_count - 1),
            Some(row) => row.saturating_sub(1),
        };
        self.selected = model.row_process(next).map(|process| process.uniqueid);
        // Keep it in view.
        let visible = layout.visible_rows().max(1);
        if next < self.first_row {
            self.first_row = next;
        } else if next >= self.first_row + visible {
            self.first_row = next + 1 - visible;
        }
        true
    }

    fn hit(&self, layout: &Layout, point: Point) -> Option<Hover> {
        if let Some(index) = layout.segments.iter().position(|rect| rect.contains(point)) {
            return Some(Hover::Segment(index));
        }
        if layout.header.contains(point) {
            return layout
                .columns
                .iter()
                .position(|rect| rect.size.width > 0 && point.x >= rect.origin.x && point.x < rect.origin.x + rect.size.width as i32)
                .map(Hover::Header);
        }
        None
    }

    fn draw_toolbar(&self, ui: &mut Frame<'_>, layout: &Layout) {
        let model = model();
        ui.fill_rect(layout.toolbar, TOOLBAR_BACKGROUND);
        ui.fill_rect(Rect::new(0, layout.toolbar.size.height as i32 - 1, layout.toolbar.size.width, 1), DIVIDER);

        // Title and live subtitle, macOS 11-style.
        let title_size = scale::pt(13);
        let title_h = ui.measure("Ag", title_size).height as i32;
        let caption_h = ui.measure("Ag", caption_size()).height as i32;
        let block = title_h + caption_h;
        let top = (layout.toolbar.size.height as i32 - block) / 2;
        ui.text_semibold(Point::new(margin(), top), self.filter.title(), system_color::LABEL, title_size);

        let mut subtitle = model::Text::<64>::new();
        use core::fmt::Write;
        if model.available {
            let _ = write!(
                subtitle,
                "{} processes, {} threads",
                model.activity.process_count,
                model.thread_count(),
            );
        } else {
            let _ = subtitle.write_str("No data from this system");
        }
        ui.text(Point::new(margin(), top + title_h), subtitle.as_str(), system_color::SECONDARY_LABEL, caption_size());

        // The filter as a segmented control.
        let track = Rect::new(
            layout.segments[0].origin.x,
            layout.segments[0].origin.y,
            layout.segments[0].size.width * 3,
            layout.segments[0].size.height,
        );
        ui.fill_rounded_rect(track, scale::pt(6), SEGMENT_TRACK);
        for (index, rect) in layout.segments.iter().enumerate() {
            let selected = FILTERS[index] == self.filter;
            let inset = scale::pt(2) as i32;
            let puck = Rect::new(rect.origin.x + inset, rect.origin.y + inset, rect.size.width - inset as u32 * 2, rect.size.height - inset as u32 * 2);
            if selected {
                ui.fill_rounded_rect(puck, scale::pt(5), SEGMENT_PUCK);
            } else if self.hovered == Some(Hover::Segment(index)) {
                ui.fill_rounded_rect(puck, scale::pt(5), SEGMENT_HOVER);
            }
            let label = FILTER_SEGMENTS[index];
            let measured = ui.measure_with_weight(label, label_size(), if selected { FontWeight::Semibold } else { FontWeight::Regular });
            let x = rect.origin.x + (rect.size.width as i32 - measured.width as i32) / 2;
            let y = rect.origin.y + (rect.size.height as i32 - measured.height as i32) / 2;
            ui.text_with_weight(
                Point::new(x, y),
                label,
                system_color::LABEL,
                label_size(),
                if selected { FontWeight::Semibold } else { FontWeight::Regular },
            );
        }
    }

    fn draw_header(&self, ui: &mut Frame<'_>, layout: &Layout) {
        ui.fill_rect(layout.header, HEADER_BACKGROUND);
        ui.fill_rect(Rect::new(0, layout.header.origin.y + layout.header.size.height as i32 - 1, layout.header.size.width, 1), DIVIDER);

        for (index, &column) in Column::ALL.iter().enumerate() {
            let rect = layout.columns[index];
            if rect.size.width == 0 {
                continue;
            }
            if self.hovered == Some(Hover::Header(index)) {
                ui.fill_rect(Rect::new(rect.origin.x, rect.origin.y, rect.size.width, rect.size.height - 1), HEADER_HOVER);
            }
            // Column separators, like a table view's.
            if index > 0 {
                let inset = scale::pt(4) as i32;
                ui.fill_rect(Rect::new(rect.origin.x, rect.origin.y + inset, 1, rect.size.height.saturating_sub(inset as u32 * 2)), HAIRLINE);
            }

            let sorted = column == self.sort;
            let weight = if sorted { FontWeight::Semibold } else { FontWeight::Regular };
            let title = column.title();
            let measured = ui.measure_with_weight(title, caption_size(), weight);
            let y = rect.origin.y + (rect.size.height as i32 - measured.height as i32) / 2;
            let chevron_room = if sorted { scale::pt(12) as i32 } else { 0 };
            let x = if right_aligned(column) {
                rect.origin.x + rect.size.width as i32 - cell_padding() - measured.width as i32 - chevron_room
            } else {
                rect.origin.x + cell_padding()
            };
            ui.text_with_weight(Point::new(x, y), title, system_color::LABEL, caption_size(), weight);
            if sorted {
                draw_chevron(ui, Point::new(x + measured.width as i32 + scale::pt(4) as i32, rect.origin.y + rect.size.height as i32 / 2), self.descending);
            }
        }
    }

    fn draw_rows(&self, ui: &mut Frame<'_>, layout: &Layout) {
        let model = model();
        ui.fill_rect(layout.rows, Color::WHITE);

        if !model.available {
            let message = "Activity Monitor needs a system that reports its processes.";
            let measured = ui.measure(message, label_size());
            let x = (layout.rows.size.width as i32 - measured.width as i32) / 2;
            let y = layout.rows.origin.y + (layout.rows.size.height as i32 - measured.height as i32) / 2;
            ui.text(Point::new(x, y), message, system_color::SECONDARY_LABEL, label_size());
            return;
        }

        let hz = model.tick_hz();
        let visible = layout.visible_rows() + 1;
        let row_h = row_height();
        for slot in 0..visible {
            let row_index = self.first_row + slot;
            let y = layout.rows.origin.y + (slot as u32 * row_h) as i32;
            if y >= layout.rows.origin.y + layout.rows.size.height as i32 {
                break;
            }
            // The last partial row must not paint over the panel.
            let height = row_h.min((layout.rows.origin.y + layout.rows.size.height as i32 - y) as u32);
            let rect = Rect::new(0, y, layout.rows.size.width, height);

            let Some(row) = model.rows[..model.row_count].get(row_index).copied() else {
                if row_index % 2 == 1 {
                    ui.fill_rect(rect, STRIPE);
                }
                continue;
            };
            let process = model.processes[row.process];
            let selected = self.selected == Some(process.uniqueid);
            if selected {
                ui.fill_rect(rect, system_color::ACCENT);
            } else if row_index % 2 == 1 {
                ui.fill_rect(rect, STRIPE);
            }
            if height < row_h {
                continue;
            }

            let primary = if selected { system_color::ON_ACCENT } else { system_color::LABEL };
            let secondary = if selected { system_color::ON_ACCENT } else { system_color::SECONDARY_LABEL };

            for (index, &column) in Column::ALL.iter().enumerate() {
                if layout.columns[index].size.width == 0 {
                    continue;
                }
                let cell = Rect::new(layout.columns[index].origin.x, y, layout.columns[index].size.width, row_h);
                match column {
                    Column::Name => {
                        let icon = Rect::new(cell.origin.x + cell_padding(), y + (row_h - scale::pt(12)) as i32 / 2, scale::pt(12), scale::pt(12));
                        draw_process_icon(ui, icon, &process, selected);
                        let x = icon.origin.x + icon.size.width as i32 + scale::pt(6) as i32;
                        let room = (cell.origin.x + cell.size.width as i32 - cell_padding() - x).max(0) as u32;
                        let name = if process.name().is_empty() { "(unnamed)" } else { process.name() };
                        let mut scratch = [0u8; 48];
                        let fitted = fit(ui, name, label_size(), room, &mut scratch);
                        cell_text(ui, cell, fitted, primary, false, Some(x));
                    }
                    Column::Cpu => {
                        let text = if model.samples >= 2 { model::percent(row.cpu_permille) } else { text_of("--") };
                        cell_text(ui, cell, text.as_str(), primary, true, None);
                    }
                    Column::CpuTime => cell_text(ui, cell, model::cpu_time(process.cpu_ticks, hz).as_str(), primary, true, None),
                    Column::Threads => cell_text(ui, cell, model::number(u64::from(process.threads)).as_str(), primary, true, None),
                    Column::State => cell_text(ui, cell, model::state_title(process.state), secondary, false, None),
                    Column::Queue => {
                        let mut text = model::Text::<8>::new();
                        use core::fmt::Write;
                        let _ = write!(text, "Q{}", process.mlfq_level);
                        cell_text(ui, cell, text.as_str(), secondary, true, None);
                    }
                    Column::LastCpu => cell_text(ui, cell, model::number(u64::from(process.last_cpu)).as_str(), secondary, true, None),
                    Column::Pid => cell_text(ui, cell, model::number(u64::from(process.pid)).as_str(), secondary, true, None),
                }
            }
        }
    }

    fn draw_panel(&self, ui: &mut Frame<'_>, layout: &Layout) {
        let model = model();
        let panel = layout.panel;
        if panel.size.height < scale::pt(40) {
            return;
        }
        ui.fill_rect(panel, PANEL_BACKGROUND);
        ui.fill_rect(Rect::new(0, panel.origin.y, panel.size.width, 1), DIVIDER);

        let width = panel.size.width;
        let side = (width * 2 / 7).clamp(scale::pt(130), scale::pt(190));
        let left = Rect::new(0, panel.origin.y, side, panel.size.height);
        let right = Rect::new((width - side) as i32, panel.origin.y, side, panel.size.height);
        let middle = Rect::new(side as i32, panel.origin.y, width - side * 2, panel.size.height);
        ui.fill_rect(Rect::new(middle.origin.x, panel.origin.y + scale::pt(10) as i32, 1, panel.size.height - scale::pt(20)), HAIRLINE);
        ui.fill_rect(Rect::new(right.origin.x, panel.origin.y + scale::pt(10) as i32, 1, panel.size.height - scale::pt(20)), HAIRLINE);

        // Left: the machine's load.
        let busy = model.busy_permille.min(1000);
        let busy_text = if model.samples >= 2 { percent_sign(busy) } else { text_of("--") };
        let idle_text = if model.samples >= 2 { percent_sign(1000 - busy) } else { text_of("--") };
        let uptime_text = model::uptime(model.activity.uptime_us);
        let cpus_text = model::number(u64::from(model.activity.cpu_count));
        let lines_left: [(&str, &str); 4] = [
            ("Busy:", busy_text.as_str()),
            ("Idle:", idle_text.as_str()),
            ("CPUs:", cpus_text.as_str()),
            ("Uptime:", uptime_text.as_str()),
        ];
        draw_stats(ui, left, &lines_left);

        // Middle: one load graph per CPU.
        let cpus = (model.activity.cpu_count as usize).clamp(1, activity::CPU_MAX);
        let caption = "CPU LOAD";
        let caption_measured = ui.measure(caption, caption_size());
        let caption_y = middle.origin.y + scale::pt(8) as i32;
        ui.text(
            Point::new(middle.origin.x + (middle.size.width as i32 - caption_measured.width as i32) / 2, caption_y),
            caption,
            system_color::SECONDARY_LABEL,
            caption_size(),
        );
        let graphs_top = caption_y + caption_measured.height as i32 + scale::pt(6) as i32;
        let graphs_bottom = middle.origin.y + middle.size.height as i32 - scale::pt(10) as i32;
        let gap = scale::pt(6);
        let inner = middle.size.width.saturating_sub(scale::pt(24));
        let graph_w = (inner.saturating_sub(gap * (cpus as u32 - 1))) / cpus as u32;
        let graph_h = (graphs_bottom - graphs_top).max(0) as u32;
        let graphs_left = middle.origin.x + scale::pt(12) as i32;
        for cpu in 0..cpus {
            let rect = Rect::new(graphs_left + ((graph_w + gap) * cpu as u32) as i32, graphs_top, graph_w, graph_h);
            let online = model.activity.cpus[cpu].online != 0;
            draw_graph(ui, rect, model.history(cpu), cpu, online);
        }

        // Right: processes, threads, memory.
        let page = model.activity.page_size;
        let total = model.activity.total_pages * page;
        let used = model.activity.total_pages.saturating_sub(model.activity.free_pages) * page;
        let processes = model::number(u64::from(model.activity.process_count));
        let threads = model::number(u64::from(model.thread_count()));
        let used_text = model::bytes(used);
        let total_text = model::bytes(total);
        let lines_right: [(&str, &str); 4] = [
            ("Processes:", processes.as_str()),
            ("Threads:", threads.as_str()),
            ("Memory:", used_text.as_str()),
            ("Physical:", total_text.as_str()),
        ];
        draw_stats(ui, right, &lines_right);
    }
}

impl Default for ActivityMonitorApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Tenths of a percent as "12.3%".
fn percent_sign(permille: u32) -> model::Text<16> {
    let mut out = model::Text::new();
    use core::fmt::Write;
    let _ = write!(out, "{}%", model::percent(permille).as_str());
    out
}

fn text_of(text: &str) -> model::Text<16> {
    let mut out = model::Text::new();
    use core::fmt::Write;
    let _ = out.write_str(text);
    out
}

/// `text` shortened with an ellipsis to fit `max_width`, copied into `scratch`.
fn fit<'a>(ui: &Frame<'_>, text: &'a str, size: u32, max_width: u32, scratch: &'a mut [u8]) -> &'a str {
    if ui.measure(text, size).width <= max_width {
        return text;
    }
    const ELLIPSIS: &str = "\u{2026}";
    let mut end = text.len();
    while end > 0 {
        end -= 1;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let head = &text[..end];
        if head.len() + ELLIPSIS.len() > scratch.len() {
            continue;
        }
        scratch[..head.len()].copy_from_slice(head.as_bytes());
        scratch[head.len()..head.len() + ELLIPSIS.len()].copy_from_slice(ELLIPSIS.as_bytes());
        let candidate = core::str::from_utf8(&scratch[..head.len() + ELLIPSIS.len()]).unwrap_or("");
        if ui.measure(candidate, size).width <= max_width {
            let length = candidate.len();
            return core::str::from_utf8(&scratch[..length]).unwrap_or("");
        }
    }
    ""
}

/// One cell's text, vertically centred; figures right-aligned.
fn cell_text(ui: &mut Frame<'_>, cell: Rect, text: &str, color: Color, right: bool, x: Option<i32>) {
    let measured = ui.measure(text, label_size());
    let y = cell.origin.y + (cell.size.height as i32 - measured.height as i32) / 2;
    let x = x.unwrap_or(if right {
        cell.origin.x + cell.size.width as i32 - cell_padding() - measured.width as i32
    } else {
        cell.origin.x + cell_padding()
    });
    ui.text(Point::new(x, y), text, color, label_size());
}

/// "Label: value" lines, labels right-aligned against a shared column.
fn draw_stats(ui: &mut Frame<'_>, area: Rect, lines: &[(&str, &str)]) {
    let line_h = ui.measure("Ag", label_size()).height as i32 + scale::pt(3) as i32;
    let block = line_h * lines.len() as i32;
    let top = area.origin.y + (area.size.height as i32 - block) / 2;
    let label_w = lines.iter().map(|(label, _)| ui.measure(label, label_size()).width).max().unwrap_or(0) as i32;
    let x = area.origin.x + margin();
    for (index, (label, value)) in lines.iter().enumerate() {
        let y = top + line_h * index as i32;
        let measured = ui.measure(label, label_size());
        ui.text(Point::new(x + label_w - measured.width as i32, y), label, system_color::SECONDARY_LABEL, label_size());
        ui.text_semibold(Point::new(x + label_w + scale::pt(6) as i32, y), value, system_color::LABEL, label_size());
    }
}

/// A CPU's load history as a bar chart, newest on the right.
fn draw_graph(ui: &mut Frame<'_>, rect: Rect, history: &[u8], cpu: usize, online: bool) {
    if rect.size.width < 4 || rect.size.height < 4 {
        return;
    }
    ui.fill_rounded_rect(rect, scale::pt(4), GRAPH_BACKGROUND);
    // Quarter lines.
    for quarter in 1..4 {
        let y = rect.origin.y + (rect.size.height * quarter / 4) as i32;
        ui.fill_rect(Rect::new(rect.origin.x + 2, y, rect.size.width - 4, 1), GRAPH_GRID);
    }

    let inner_h = rect.size.height.saturating_sub(4);
    let bar_w = (rect.size.width / model::HISTORY as u32).max(1);
    let shown = (rect.size.width / bar_w) as usize;
    let start = history.len().saturating_sub(shown);
    let right = rect.origin.x + rect.size.width as i32 - 2;
    for (age, &load) in history[start..].iter().rev().enumerate() {
        let x = right - ((age as u32 + 1) * bar_w) as i32;
        if x < rect.origin.x + 2 {
            break;
        }
        let h = inner_h * u32::from(load.min(100)) / 100;
        if h == 0 {
            continue;
        }
        let y = rect.origin.y + 2 + (inner_h - h) as i32;
        ui.fill_rect(Rect::new(x, y, bar_w, h), GRAPH_LOAD);
        ui.fill_rect(Rect::new(x, y, bar_w, 1.min(h)), GRAPH_LOAD_TOP);
    }

    let mut label = model::Text::<16>::new();
    {
        use core::fmt::Write;
        let _ = write!(label, "{}{}", cpu, if online { "" } else { " off" });
    }
    ui.text(
        Point::new(rect.origin.x + scale::pt(4) as i32, rect.origin.y + scale::pt(2) as i32),
        label.as_str(),
        Color::rgb(150, 155, 165),
        scale::pt(9),
    );
}

/// A small sort chevron centred on `center`: down for descending.
fn draw_chevron(ui: &mut Frame<'_>, center: Point, down: bool) {
    let unit = scale::pt(1).max(1) as i32;
    for step in 0..4 {
        let half = (3 - step) * unit;
        let y = if down { center.y - 2 * unit + step * unit } else { center.y + unit - step * unit };
        ui.fill_rect(Rect::new(center.x - half, y, (half * 2 + unit) as u32, unit as u32), system_color::SECONDARY_LABEL);
    }
}

/// A rounded square badge: gear-grey for the kernel, blue for the system's
/// own daemons, green for everything else, with the name's first letter.
fn draw_process_icon(ui: &mut Frame<'_>, rect: Rect, process: &activity::ProcessInfo, selected: bool) {
    let color = if process.flags & activity::FLAG_KERNEL != 0 {
        Color::rgb(120, 124, 134)
    } else if process.flags & activity::FLAG_SYSTEM != 0 {
        Color::rgb(64, 120, 230)
    } else {
        Color::rgb(46, 170, 92)
    };
    ui.fill_rounded_rect(rect, scale::pt(3), if selected { Color::WHITE } else { color });
    let initial = process.name().as_bytes().first().copied().unwrap_or(b'?').to_ascii_uppercase();
    let mut letter = [0u8; 1];
    letter[0] = if initial.is_ascii_graphic() { initial } else { b'?' };
    let letter = core::str::from_utf8(&letter).unwrap_or("?");
    let size = scale::pt(9);
    let measured = ui.measure_with_weight(letter, size, FontWeight::Semibold);
    ui.text_with_weight(
        Point::new(
            rect.origin.x + (rect.size.width as i32 - measured.width as i32) / 2,
            rect.origin.y + (rect.size.height as i32 - measured.height as i32) / 2,
        ),
        letter,
        if selected { system_color::ACCENT } else { Color::WHITE },
        size,
        FontWeight::Semibold,
    );
}

const VIEW_MENU: &[MenuItem] = &[
    MenuItem::command("All Processes", COMMAND_FILTER_ALL),
    MenuItem::command("User Processes", COMMAND_FILTER_USER),
    MenuItem::command("System Processes", COMMAND_FILTER_SYSTEM),
    MenuItem::SEPARATOR,
    MenuItem::command("Update Every Second", COMMAND_INTERVAL_1),
    MenuItem::command("Update Every 2 Seconds", COMMAND_INTERVAL_2),
    MenuItem::command("Update Every 5 Seconds", COMMAND_INTERVAL_5),
    MenuItem::SEPARATOR,
    MenuItem::command("Update Now", COMMAND_SAMPLE_NOW),
];

impl App for ActivityMonitorApp {
    const INFO: AppInfo<'static> = AppInfo::new("Activity Monitor", "com.butterscotch.activity-monitor", "0.1.0");
    const WINDOW: WindowConfig = WindowConfig::new(WINDOW_WIDTH, WINDOW_HEIGHT)
        .resizable(Size::new(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT), None);
    const MENUS: &'static [Menu] = &[Menu::new("View", VIEW_MENU)];

    fn draw(&mut self, ui: &mut Frame<'_>) {
        let size = ui.size();
        self.last_size = size;
        let layout = Layout::compute(size);
        self.clamp_scroll(&layout);
        self.draw_rows(ui, &layout);
        self.draw_header(ui, &layout);
        self.draw_toolbar(ui, &layout);
        self.draw_panel(ui, &layout);
    }

    fn event(&mut self, event: Event, content_size: Size) -> AppAction {
        let layout = Layout::compute(content_size);
        match event {
            Event::PointerMoved { position } => {
                let hovered = self.hit(&layout, position);
                if hovered != self.hovered {
                    self.hovered = hovered;
                    return AppAction::Redraw;
                }
                AppAction::None
            }
            Event::PointerLeft => {
                if self.hovered.take().is_some() { AppAction::Redraw } else { AppAction::None }
            }
            Event::PointerDown { position, button: PointerButton::Primary } => {
                match self.hit(&layout, position) {
                    Some(Hover::Segment(index)) => {
                        return if self.set_filter(FILTERS[index]) { AppAction::Redraw } else { AppAction::None };
                    }
                    Some(Hover::Header(index)) => {
                        self.sort_by(Column::ALL[index]);
                        return AppAction::Redraw;
                    }
                    None => {}
                }
                if layout.rows.contains(position) {
                    let row = self.first_row + ((position.y - layout.rows.origin.y) as u32 / row_height()) as usize;
                    let selected = model().row_process(row).map(|process| process.uniqueid);
                    if selected != self.selected {
                        self.selected = selected;
                        return AppAction::Redraw;
                    }
                }
                AppAction::None
            }
            Event::Scroll { delta, .. } => {
                let before = self.first_row;
                if delta > 0 {
                    self.first_row = self.first_row.saturating_sub(delta as usize);
                } else {
                    self.first_row += delta.unsigned_abs() as usize;
                }
                self.clamp_scroll(&layout);
                if self.first_row != before { AppAction::Redraw } else { AppAction::None }
            }
            Event::KeyDown { code, .. } => match code {
                ui::core::key::UP => self.move_selection(false, &layout).then_some(AppAction::Redraw).unwrap_or_default(),
                ui::core::key::DOWN => self.move_selection(true, &layout).then_some(AppAction::Redraw).unwrap_or_default(),
                ui::core::key::ESCAPE => {
                    if self.selected.take().is_some() { AppAction::Redraw } else { AppAction::None }
                }
                _ => AppAction::None,
            },
            _ => AppAction::None,
        }
    }

    // Always: the figures move whether or not anything else happens.
    fn animating(&self) -> bool {
        true
    }

    fn tick(&mut self, now_us: u64) -> AppAction {
        if now_us < self.next_sample_us {
            return AppAction::None;
        }
        // The very first tick only schedules: the sample `new` took is a
        // moment old, and a second one straight away would measure nothing.
        let first = self.next_sample_us == 0;
        self.next_sample_us = now_us + if first { 500_000 } else { self.interval_us };
        if first {
            return AppAction::None;
        }
        model().sample();
        self.rebuild();
        AppAction::Redraw
    }

    fn menu_item_state(&self, command: u32) -> MenuItemState {
        match command {
            COMMAND_FILTER_ALL => MenuItemState::checked(self.filter == Filter::All),
            COMMAND_FILTER_USER => MenuItemState::checked(self.filter == Filter::User),
            COMMAND_FILTER_SYSTEM => MenuItemState::checked(self.filter == Filter::System),
            COMMAND_INTERVAL_1 => MenuItemState::checked(self.interval_us == 1_000_000),
            COMMAND_INTERVAL_2 => MenuItemState::checked(self.interval_us == 2_000_000),
            COMMAND_INTERVAL_5 => MenuItemState::checked(self.interval_us == 5_000_000),
            _ => MenuItemState::ENABLED,
        }
    }

    fn menu_command(&mut self, command: u32) -> AppAction {
        match command {
            COMMAND_FILTER_ALL => self.set_filter(Filter::All).then_some(AppAction::Redraw).unwrap_or_default(),
            COMMAND_FILTER_USER => self.set_filter(Filter::User).then_some(AppAction::Redraw).unwrap_or_default(),
            COMMAND_FILTER_SYSTEM => self.set_filter(Filter::System).then_some(AppAction::Redraw).unwrap_or_default(),
            COMMAND_INTERVAL_1 | COMMAND_INTERVAL_2 | COMMAND_INTERVAL_5 => {
                self.interval_us = match command {
                    COMMAND_INTERVAL_1 => 1_000_000,
                    COMMAND_INTERVAL_2 => 2_000_000,
                    _ => 5_000_000,
                };
                // Takes effect from the next sample on.
                AppAction::None
            }
            COMMAND_SAMPLE_NOW => {
                model().sample();
                self.rebuild();
                AppAction::Redraw
            }
            _ => AppAction::None,
        }
    }
}

#[cfg(test)]
mod tests;
