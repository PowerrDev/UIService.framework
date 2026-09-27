//! Host tests: the model against a fake host, and the formatting.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Mutex;

use ui::core::activity::{self, Activity, ProcessInfo};

use crate::model::{self, Column, Filter, Model};

/// `activity::set_backend` is one global: tests that touch it take turns.
static BACKEND: Mutex<()> = Mutex::new(());

thread_local! {
    static FAKE: RefCell<(Activity, Vec<ProcessInfo>)> = RefCell::new((Activity::empty(), Vec::new()));
}

unsafe extern "C" fn fake_sample(
    _context: *mut c_void,
    activity: *mut Activity,
    processes: *mut ProcessInfo,
    capacity: u32,
    count_out: *mut u32,
) -> u32 {
    FAKE.with(|fake| {
        let fake = fake.borrow();
        unsafe {
            *activity = fake.0;
            let count = fake.1.len().min(capacity as usize);
            for (index, process) in fake.1.iter().take(count).enumerate() {
                *processes.add(index) = *process;
            }
            *count_out = count as u32;
        }
    });
    0
}

fn process(uniqueid: u64, pid: u32, name: &str, ticks: u64, flags: u32) -> ProcessInfo {
    let mut process = ProcessInfo::empty().with_name(name);
    process.uniqueid = uniqueid;
    process.pid = pid;
    process.cpu_ticks = ticks;
    process.flags = flags;
    process.threads = 1;
    process.state = activity::STATE_SLEEPING;
    process
}

fn set_fake(cpu_ticks: u64, busy: u64, processes: Vec<ProcessInfo>) {
    FAKE.with(|fake| {
        let mut fake = fake.borrow_mut();
        let mut activity = Activity::empty();
        activity.cpu_count = 2;
        activity.uptime_us = cpu_ticks * 10_000; // 100 Hz
        activity.page_size = 4096;
        activity.total_pages = 1000;
        activity.free_pages = 250;
        activity.process_count = processes.len() as u32;
        for cpu in 0..2 {
            activity.cpus[cpu].ticks = cpu_ticks;
            activity.cpus[cpu].busy_ticks = busy;
            activity.cpus[cpu].online = 1;
        }
        fake.0 = activity;
        fake.1 = processes;
    });
}

fn names(model: &Model) -> Vec<String> {
    (0..model.row_count).map(|row| model.row_process(row).unwrap().name().to_string()).collect()
}

#[test]
fn activity_model_measures_cpu_between_samples() {
    let _backend = BACKEND.lock().unwrap_or_else(|poison| poison.into_inner());
    activity::set_backend(core::ptr::null_mut(), Some(fake_sample));
    let mut model = Box::new(Model::new());

    set_fake(100, 50, vec![
        process(0, 0, "kernel_task", 40, activity::FLAG_KERNEL),
        process(1, 1, "bootd", 10, activity::FLAG_SYSTEM),
        process(7, 7, "shell", 0, 0),
    ]);
    model.sample();
    model.build_rows(Filter::All, Column::Cpu, true);
    // One sample: nothing to measure against yet.
    assert!(model.rows[..model.row_count].iter().all(|row| row.cpu_permille == 0));

    // 100 more ticks per CPU; the kernel ran 30 of them, the shell 150
    // (more than one CPU's worth), and a process started in between.
    set_fake(200, 120, vec![
        process(0, 0, "kernel_task", 70, activity::FLAG_KERNEL),
        process(1, 1, "bootd", 10, activity::FLAG_SYSTEM),
        process(7, 7, "shell", 150, 0),
        process(9, 9, "logd", 5, activity::FLAG_SYSTEM),
    ]);
    model.sample();
    model.build_rows(Filter::All, Column::Cpu, true);
    assert_eq!(names(&model), ["shell", "kernel_task", "bootd", "logd"]);
    let cpu: Vec<u32> = model.rows[..model.row_count].iter().map(|row| row.cpu_permille).collect();
    assert_eq!(cpu, [1500, 300, 0, 0]);
    // Both CPUs 70 of 100 ticks busy.
    assert_eq!(model.busy_permille, 700);
    assert_eq!(model.history(0), [70]);
    assert_eq!(model.tick_hz(), 100);
}

#[test]
fn activity_model_filters_and_sorts() {
    let _backend = BACKEND.lock().unwrap_or_else(|poison| poison.into_inner());
    activity::set_backend(core::ptr::null_mut(), Some(fake_sample));
    let mut model = Box::new(Model::new());
    set_fake(100, 0, vec![
        process(0, 0, "kernel_task", 0, activity::FLAG_KERNEL),
        process(1, 1, "bootd", 0, activity::FLAG_SYSTEM),
        process(3, 12, "Zebra", 0, 0),
        process(4, 11, "apple", 0, 0),
    ]);
    model.sample();

    model.build_rows(Filter::User, Column::Name, false);
    assert_eq!(names(&model), ["apple", "Zebra"]);
    model.build_rows(Filter::System, Column::Pid, true);
    assert_eq!(names(&model), ["bootd", "kernel_task"]);
    model.build_rows(Filter::All, Column::Pid, false);
    assert_eq!(names(&model), ["kernel_task", "bootd", "apple", "Zebra"]);
}

#[test]
fn activity_model_without_a_host_is_unavailable() {
    let _backend = BACKEND.lock().unwrap_or_else(|poison| poison.into_inner());
    activity::set_backend(core::ptr::null_mut(), None);
    let mut model = Box::new(Model::new());
    model.sample();
    assert!(!model.available);
    assert_eq!(model.process_count, 0);
}

#[test]
fn activity_formatting() {
    assert_eq!(model::percent(1234).as_str(), "123.4");
    assert_eq!(model::percent(5).as_str(), "0.5");
    assert_eq!(model::cpu_time(12_345, 100).as_str(), "2:03.45");
    assert_eq!(model::cpu_time(100 * 3723, 100).as_str(), "1:02:03.00");
    assert_eq!(model::cpu_time(10, 0).as_str(), "--");
    assert_eq!(model::uptime(3_723_000_000).as_str(), "1:02:03");
    assert_eq!(model::uptime(90_061_000_000).as_str(), "1 day, 1:01:01");
    assert_eq!(model::bytes(512 * 1024 * 1024).as_str(), "512 MB");
    assert_eq!(model::bytes(3 * 1024 * 1024 * 1024 / 2).as_str(), "1.5 GB");
    assert_eq!(model::bytes(2048).as_str(), "2 KB");
}

#[test]
fn activity_process_names_survive_truncation() {
    let process = ProcessInfo::empty().with_name("a-very-long-process-name-that-goes-past-31");
    assert_eq!(process.name().len(), activity::NAME_MAX);
}

/// The desktop builds the app on the arm64 kernel's 16 KiB boot stack before
/// moving it into its static: it must stay small (the model is a static).
#[test]
fn activity_monitor_app_stays_small() {
    let size = core::mem::size_of::<crate::ActivityMonitorApp>();
    assert!(size <= 256, "ActivityMonitorApp is {size} bytes");
}
