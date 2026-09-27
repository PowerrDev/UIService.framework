//! What Activity Monitor knows: the latest sample from the host, what moved
//! since the one before it, and the rows the table shows, sorted and filtered.
//!
//! CPU figures are ratios of tick counts (see `ui::core::activity`): a
//! process's % CPU is how far its `cpu_ticks` moved against one CPU's `ticks`
//! over the same interval, so 100% is one whole CPU and a process with
//! threads on several CPUs can go past it, as on macOS. All integer math: the
//! arm64 kernel target has no FPU.

use core::fmt::Write;

use ui::core::activity::{self, Activity, CpuInfo, ProcessInfo};

/// As many processes as the kernel's process table holds (`PROC_MAX`).
pub const MAX_PROCESSES: usize = 64;
/// Load history kept per CPU: one sample per update.
pub const HISTORY: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Column {
    Name,
    Cpu,
    CpuTime,
    Threads,
    State,
    Queue,
    LastCpu,
    Pid,
}

impl Column {
    pub const ALL: [Column; 8] = [
        Column::Name,
        Column::Cpu,
        Column::CpuTime,
        Column::Threads,
        Column::State,
        Column::Queue,
        Column::LastCpu,
        Column::Pid,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Column::Name => "Process Name",
            Column::Cpu => "% CPU",
            Column::CpuTime => "CPU Time",
            Column::Threads => "Threads",
            Column::State => "State",
            Column::Queue => "Queue",
            Column::LastCpu => "CPU",
            Column::Pid => "PID",
        }
    }

    /// The direction a first click on the header sorts in: busiest,
    /// biggest first for figures, A to Z for names.
    pub const fn descending_first(self) -> bool {
        !matches!(self, Column::Name | Column::State | Column::Pid)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Filter {
    All,
    /// Processes started from a program (not the kernel's own, not flagged
    /// as the system's).
    User,
    System,
}

impl Filter {
    pub const fn title(self) -> &'static str {
        match self {
            Filter::All => "All Processes",
            Filter::User => "User Processes",
            Filter::System => "System Processes",
        }
    }

    fn admits(self, process: &ProcessInfo) -> bool {
        let system = process.flags & (activity::FLAG_KERNEL | activity::FLAG_SYSTEM) != 0;
        match self {
            Filter::All => true,
            Filter::User => !system,
            Filter::System => system,
        }
    }
}

/// One row of the table, derived from a process in the latest sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Row {
    /// Index into `Model::processes`.
    pub process: usize,
    /// % CPU over the last interval, in tenths of a percent (123 = 12.3%).
    pub cpu_permille: u32,
}

pub struct Model {
    pub activity: Activity,
    pub processes: [ProcessInfo; MAX_PROCESSES],
    pub process_count: usize,
    /// Whether the host has ever answered: `false` shows a notice instead.
    pub available: bool,
    /// Samples taken so far; % CPU needs two.
    pub samples: u64,

    previous: [(u64, u64); MAX_PROCESSES],
    previous_count: usize,
    previous_cpus: [CpuInfo; activity::CPU_MAX],

    pub rows: [Row; MAX_PROCESSES],
    pub row_count: usize,

    /// % busy per CPU, oldest first once full: `history[cpu][..history_len]`.
    history: [[u8; HISTORY]; activity::CPU_MAX],
    history_len: usize,
    /// Whole-machine busy share over the last interval, tenths of a percent.
    pub busy_permille: u32,
}

impl Model {
    pub const fn new() -> Self {
        Self {
            activity: Activity::empty(),
            processes: [ProcessInfo::empty(); MAX_PROCESSES],
            process_count: 0,
            available: false,
            samples: 0,
            previous: [(0, 0); MAX_PROCESSES],
            previous_count: 0,
            previous_cpus: [CpuInfo { ticks: 0, busy_ticks: 0, context_switches: 0, online: 0, reserved: 0 }; activity::CPU_MAX],
            rows: [Row { process: 0, cpu_permille: 0 }; MAX_PROCESSES],
            row_count: 0,
            history: [[0; HISTORY]; activity::CPU_MAX],
            history_len: 0,
            busy_permille: 0,
        }
    }

    /// Back to `new()`'s state, in place: `*model = Model::new()` would build
    /// the whole ~15 KiB model as a temporary on the caller's stack first.
    pub fn reset(&mut self) {
        self.activity = Activity::empty();
        self.processes.fill(ProcessInfo::empty());
        self.process_count = 0;
        self.available = false;
        self.samples = 0;
        self.previous.fill((0, 0));
        self.previous_count = 0;
        self.previous_cpus = [CpuInfo::default(); activity::CPU_MAX];
        self.rows.fill(Row { process: 0, cpu_permille: 0 });
        self.row_count = 0;
        for history in &mut self.history {
            history.fill(0);
        }
        self.history_len = 0;
        self.busy_permille = 0;
    }

    /// Take a new sample from the host, keeping the old one's tick counts to
    /// measure against.
    pub fn sample(&mut self) {
        // Remember where every process and CPU stood.
        self.previous_count = self.process_count;
        for index in 0..self.process_count {
            let process = &self.processes[index];
            self.previous[index] = (process.uniqueid, process.cpu_ticks);
        }
        self.previous_cpus = self.activity.cpus;
        let had_sample = self.available;

        match activity::sample(&mut self.activity, &mut self.processes) {
            Ok(count) => {
                self.process_count = count;
                self.available = true;
                self.samples += 1;
            }
            Err(_) => {
                self.available = false;
                self.process_count = 0;
                return;
            }
        }

        if had_sample {
            self.record_history();
        }
    }

    /// Ticks one CPU took since the last sample: the yardstick for % CPU.
    /// The busiest-counting online CPU, in case the boot CPU's clock stalled.
    fn reference_ticks(&self) -> u64 {
        let cpus = self.activity.cpu_count.min(activity::CPU_MAX as u32) as usize;
        (0..cpus)
            .map(|cpu| self.activity.cpus[cpu].ticks.saturating_sub(self.previous_cpus[cpu].ticks))
            .max()
            .unwrap_or(0)
    }

    fn record_history(&mut self) {
        let cpus = self.activity.cpu_count.min(activity::CPU_MAX as u32) as usize;
        let mut busy_total = 0u64;
        let mut ticks_total = 0u64;

        if self.history_len == HISTORY {
            for cpu in 0..activity::CPU_MAX {
                self.history[cpu].copy_within(1.., 0);
            }
            self.history_len -= 1;
        }

        for cpu in 0..cpus {
            let now = self.activity.cpus[cpu];
            let before = self.previous_cpus[cpu];
            let ticks = now.ticks.saturating_sub(before.ticks);
            let busy = now.busy_ticks.saturating_sub(before.busy_ticks).min(ticks);
            busy_total += busy;
            ticks_total += ticks;
            self.history[cpu][self.history_len] = if ticks == 0 { 0 } else { (busy * 100 / ticks) as u8 };
        }
        self.history_len += 1;
        self.busy_permille = if ticks_total == 0 { 0 } else { (busy_total * 1000 / ticks_total) as u32 };
    }

    /// `cpu`'s load history, oldest first.
    pub fn history(&self, cpu: usize) -> &[u8] {
        &self.history[cpu.min(activity::CPU_MAX - 1)][..self.history_len]
    }

    /// % CPU of `process` over the last interval, in tenths of a percent;
    /// 0 for one that was not in the previous sample.
    fn cpu_permille(&self, process: &ProcessInfo, reference: u64) -> u32 {
        if reference == 0 {
            return 0;
        }
        let before = self.previous[..self.previous_count]
            .iter()
            .find(|(uniqueid, _)| *uniqueid == process.uniqueid)
            .map(|&(_, ticks)| ticks);
        match before {
            // A thread that exited takes its ticks with it: never negative.
            Some(before) => (process.cpu_ticks.saturating_sub(before) * 1000 / reference) as u32,
            None => 0,
        }
    }

    /// Rebuild `rows` from the latest sample, filtered and sorted.
    pub fn build_rows(&mut self, filter: Filter, column: Column, descending: bool) {
        let reference = self.reference_ticks();
        let mut count = 0;
        for index in 0..self.process_count {
            let process = self.processes[index];
            if !filter.admits(&process) {
                continue;
            }
            let cpu_permille = if self.samples >= 2 { self.cpu_permille(&process, reference) } else { 0 };
            self.rows[count] = Row { process: index, cpu_permille };
            count += 1;
        }
        self.row_count = count;

        // Insertion sort: at most 64 rows, no allocation, and stable, so
        // equal rows keep the kernel's (creation) order between updates.
        let processes = &self.processes;
        let rows = &mut self.rows[..count];
        for next in 1..rows.len() {
            let mut at = next;
            while at > 0 && ordered_before(processes, &rows[at], &rows[at - 1], column, descending) {
                rows.swap(at, at - 1);
                at -= 1;
            }
        }
    }

    pub fn row_process(&self, row: usize) -> Option<&ProcessInfo> {
        self.rows[..self.row_count].get(row).map(|row| &self.processes[row.process])
    }

    pub fn row_of(&self, uniqueid: u64) -> Option<usize> {
        self.rows[..self.row_count].iter().position(|row| self.processes[row.process].uniqueid == uniqueid)
    }

    /// Threads across every process in the sample.
    pub fn thread_count(&self) -> u32 {
        self.activity.thread_count
    }

    /// The scheduler's tick rate, measured: ticks per second of uptime.
    pub fn tick_hz(&self) -> u64 {
        let ticks = self.activity.cpus[0].ticks;
        if self.activity.uptime_us == 0 || ticks == 0 {
            return 0;
        }
        (ticks * 1_000_000 / self.activity.uptime_us).max(1)
    }
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether row `a` goes above row `b` sorting by `column`.
fn ordered_before(processes: &[ProcessInfo], a: &Row, b: &Row, column: Column, descending: bool) -> bool {
    let pa = &processes[a.process];
    let pb = &processes[b.process];
    let order = match column {
        Column::Name => compare_names(pa.name(), pb.name()),
        Column::Cpu => a.cpu_permille.cmp(&b.cpu_permille),
        Column::CpuTime => pa.cpu_ticks.cmp(&pb.cpu_ticks),
        Column::Threads => pa.threads.cmp(&pb.threads),
        Column::State => state_rank(pa.state).cmp(&state_rank(pb.state)),
        Column::Queue => pa.mlfq_level.cmp(&pb.mlfq_level),
        Column::LastCpu => pa.last_cpu.cmp(&pb.last_cpu),
        Column::Pid => pa.pid.cmp(&pb.pid),
    };
    let order = if descending { order.reverse() } else { order };
    order == core::cmp::Ordering::Less
}

/// Case-insensitive, the way a person reads a list of names.
fn compare_names(a: &str, b: &str) -> core::cmp::Ordering {
    let lower = |byte: &u8| byte.to_ascii_lowercase();
    a.as_bytes().iter().map(lower).cmp(b.as_bytes().iter().map(lower))
}

/// Running first, then ready, sleeping, stopped.
fn state_rank(state: u32) -> u32 {
    state.min(activity::STATE_OTHER)
}

pub const fn state_title(state: u32) -> &'static str {
    match state {
        activity::STATE_RUNNING => "Running",
        activity::STATE_RUNNABLE => "Ready",
        activity::STATE_SLEEPING => "Sleeping",
        activity::STATE_STOPPED => "Stopped",
        _ => "Starting",
    }
}

/// A fixed-size text buffer for formatting numbers without an allocator.
pub struct Text<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Text<N> {
    pub const fn new() -> Self {
        Self { bytes: [0; N], len: 0 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> Default for Text<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Write for Text<N> {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let room = N - self.len;
        let take = text.len().min(room);
        self.bytes[self.len..self.len + take].copy_from_slice(&text.as_bytes()[..take]);
        self.len += take;
        if take < text.len() { Err(core::fmt::Error) } else { Ok(()) }
    }
}

/// Tenths of a percent as "12.3".
pub fn percent(permille: u32) -> Text<16> {
    let mut text = Text::new();
    let _ = write!(text, "{}.{}", permille / 10, permille % 10);
    text
}

/// Ticks of CPU time as macOS shows it: "1:23.45" (minutes, seconds,
/// hundredths), or "1:02:03.45" past an hour. "--" when the rate is unknown.
pub fn cpu_time(ticks: u64, hz: u64) -> Text<24> {
    let mut text = Text::new();
    if hz == 0 {
        let _ = text.write_str("--");
        return text;
    }
    let hundredths = ticks * 100 / hz;
    let seconds = hundredths / 100;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        let _ = write!(text, "{}:{:02}:{:02}.{:02}", hours, minutes, seconds, hundredths % 100);
    } else {
        let _ = write!(text, "{}:{:02}.{:02}", minutes, seconds, hundredths % 100);
    }
    text
}

/// Microseconds of uptime as "1:02:03" (or "3 days, 1:02:03").
pub fn uptime(microseconds: u64) -> Text<32> {
    let mut text = Text::new();
    let seconds = microseconds / 1_000_000;
    let days = seconds / 86_400;
    let (hours, minutes, seconds) = (seconds / 3600 % 24, seconds / 60 % 60, seconds % 60);
    if days > 0 {
        let _ = write!(text, "{} day{}, ", days, if days == 1 { "" } else { "s" });
    }
    let _ = write!(text, "{}:{:02}:{:02}", hours, minutes, seconds);
    text
}

/// A byte count as "512 MB", "1.5 GB" or "740 KB".
pub fn bytes(value: u64) -> Text<24> {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    let mut text = Text::new();
    if value >= GB {
        let tenths = value * 10 / GB;
        let _ = write!(text, "{}.{} GB", tenths / 10, tenths % 10);
    } else if value >= MB {
        let _ = write!(text, "{} MB", value / MB);
    } else {
        let _ = write!(text, "{} KB", value / KB);
    }
    text
}

pub fn number(value: u64) -> Text<24> {
    let mut text = Text::new();
    let _ = write!(text, "{}", value);
    text
}
