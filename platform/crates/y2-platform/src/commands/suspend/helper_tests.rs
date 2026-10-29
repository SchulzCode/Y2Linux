//! The product sleep helper's flows with real lock ownership, policy refusals and the PM writes
//! (tests/test_product_auto_sleep.py, tests/test_pm_control_write.py and the helper's part of
//! tests/test_sleep_product.py). Reborn is a fake control socket in the fixture root; the kernel
//! entry, `time.sleep`, `time.monotonic`, `control_write` and the commands are this thread's seams.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    execute, kernel_sleep, operation_locks, screen_first, seams, APPLICATION_READY_WAIT_S,
    STALE_COUNT_RETRIES,
};
use crate::commands::{
    reports::workload::Lease,
    suspend::{
        admission::{admission, battery_and_cable, identity, operation_state, GUARD, RETRY},
        receipt::{record, Stage, RECEIPT},
        request,
        test_support::{answer, ready, refusal, Reborn},
        wall, E,
    },
};
use crate::reply::Failure;
use crate::testing::Fixture;
use serde_json::{json, Map, Value};
use std::{
    cell::{Cell, RefCell},
    fs::{self, File, OpenOptions},
    os::fd::AsRawFd,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard},
};
use y2p_core::{clock, command::Outcome, Root};

/// The helper's wake-count read arms the process's one ITIMER_REAL: one helper run at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn os_error(errno: i32, text: &str) -> E {
    E::Os {
        class: "OSError",
        text: format!("[Errno {errno}] {text}"),
        errno: Some(errno),
    }
}

fn done() -> Outcome {
    Outcome {
        ok: true,
        reason: None,
        output: Some(String::new()),
    }
}

/// The fixture tree as the seams and the fake Reborn change it (from any thread).
#[derive(Clone)]
struct Tree {
    base: PathBuf,
    /// (Power, RTC) serviced IRQ counts.
    counts: Arc<Mutex<(u32, u32)>>,
}

impl Tree {
    fn write(&self, name: &str, text: &str) {
        let path = self.base.join(name.trim_start_matches('/'));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn irqs(&self) {
        let (power, rtc) = *self.counts.lock().unwrap();
        self.write(
            "/proc/interrupts",
            &format!(
                "  66: {rtc} 0 mt6323 20 Level mt6397-rtc\n  70: {power} 0 mt6323 5 Level mtk-pmic-keys\n"
            ),
        );
    }

    fn press(&self) {
        self.counts.lock().unwrap().0 += 1;
        self.irqs();
    }

    fn alarm(&self) {
        self.counts.lock().unwrap().1 += 1;
        self.irqs();
    }

    fn spm(&self, timer: bool, ticks: u64) {
        self.write(
            "/sys/bus/platform/devices/10006000.power-controller/state",
            &format!(
                "system_sleep_abi=1 broken=0 timer_wake={} ticks32k={ticks}",
                u8::from(timer)
            ),
        );
    }

    /// The kernel came back: the maintenance timer, the RTC or Power.
    fn returned(&self, timer: bool, rtc: bool) {
        if !timer {
            if rtc {
                self.counts.lock().unwrap().1 += 1;
            } else {
                self.counts.lock().unwrap().0 += 1;
            }
        }
        self.irqs();
        self.spm(timer, (if timer { 600 } else { 25 }) * 32768);
        self.write(
            "/sys/firmware/y2_pm/state",
            &format!(
                "valid=1 stage=EXIT error=0 wake={} reset_resume_entry=0x59325253 pcm_length=596",
                if timer { "0x1" } else { "0x20" }
            ),
        );
    }

    fn power(&self) {
        self.write(
            "/run/y2/power.json",
            &json!({"boot_id": "same-boot", "state": "Normal", "monotonic_ns": clock::monotonic_ns()})
                .to_string(),
        );
    }

    fn prepare_refusal_ring(&self, extra: &[&str]) {
        let mut marks = vec![
            "HELPER_REQUEST",
            "SUSPEND_REQUEST",
            "BACKSTOP_STARTED",
            "FILESYSTEM_SYNCED",
        ];
        marks.extend_from_slice(extra);
        let mut lines = vec!["ring=valid cycle=50 last=1976 backstop_s=0".to_owned()];
        for (i, stage) in marks.iter().enumerate() {
            lines.push(format!(
                "{} phase=254 mark result=0 ms=297939 stage={stage}",
                1972 + i
            ));
        }
        lines.push("1976 phase=254 mark result=-1 ms=298617 stage=EXIT".into());
        self.write("/sys/firmware/y2_pm/devices", &lines.join("\n"));
        self.write(
            "/sys/firmware/y2_pm/state",
            "valid=1 stage=EXIT failed_stage=EXIT error=-1",
        );
    }
}

/// `ProductAutoSleep.setUp`: a qualified player on battery, the request's lock held, every command
/// answered ok and Reborn ready.
struct Player {
    // Dropped in this order: Reborn stops before its fixture goes.
    reborn: Reborn,
    lock: File,
    tree: Tree,
    root: Root,
    fixture: Fixture,
    calls: Rc<RefCell<Vec<Vec<String>>>>,
    _serial: MutexGuard<'static, ()>,
}

impl Player {
    fn new() -> Player {
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        seams::reset();
        let fixture = Fixture::new();
        for (name, text) in [
            ("/proc/sys/kernel/random/boot_id", "same-boot"),
            ("/proc/sys/kernel/osrelease", "corrected-kernel"),
            ("/proc/sys/kernel/tainted", "0"),
            (
                "/etc/y2linux/versions.json",
                r#"{"build_git_commit":"corrected-source"}"#,
            ),
            ("/sys/power/pm_test", "[none] core"),
            ("/sys/power/state", "mem"),
            ("/sys/power/wakeup_count", "8"),
            ("/sys/firmware/y2_pm/stage", ""),
            (
                "/sys/firmware/y2_pm/backstop_s",
                "provider=1 armed_s=0 running=0",
            ),
            ("/sys/class/power_supply/y2-usb-presence/online", "0"),
            ("/sys/class/power_supply/BAT0/status", "Discharging"),
            ("/sys/class/power_supply/BAT0/voltage_now", "3900000"),
            ("/sys/module/workload/parameters/leases", ""),
            (
                "/sys/devices/platform/18070000.connectivity/status",
                "powered=0 functions=0 error=0 ",
            ),
            ("/data/network/enabled", "0"),
            ("/data/bluetooth/enabled", "0"),
            ("/data/.y2data-schema", "1"),
            ("/run/y2/suspend.lock", ""),
        ] {
            fixture.put(name, text);
        }
        fixture.mkdir("/data/updates");
        fixture.copy_from_repository(
            "buildroot/board/y2/production-overlay/etc/y2linux/power-policy.json",
            "/etc/y2linux/power-policy.json",
        );
        let tree = Tree {
            base: fixture.base().to_owned(),
            counts: Arc::new(Mutex::new((2, 1))),
        };
        tree.power();
        tree.irqs();
        tree.spm(false, 0);
        tree.write("/sys/firmware/y2_pm/state", "valid=0 stage=EXIT error=0");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(fixture.path("/run/y2/suspend.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        let reborn = Reborn::ready(&fixture);
        let player = Player {
            root: fixture.root.clone(),
            fixture,
            tree,
            lock,
            reborn,
            calls: Rc::default(),
            _serial: serial,
        };
        player.answer_commands();
        player
    }

    /// `self.command`: every command recorded and ok.
    fn answer_commands(&self) {
        let calls = self.calls.clone();
        self.command(move |argv| {
            calls
                .borrow_mut()
                .push(argv.iter().map(|a| a.to_string()).collect());
            done()
        });
    }

    fn command(&self, f: impl FnMut(&[&str]) -> Outcome + 'static) {
        seams::SEAMS.with(|s| s.borrow_mut().command = Some(Box::new(f)));
    }

    fn enter(&self, f: impl FnMut(&Root) -> Result<(), E> + 'static) {
        seams::SEAMS.with(|s| s.borrow_mut().enter = Some(Box::new(f)));
    }

    /// The kernel must not be entered.
    fn never_enter(&self, why: &'static str) {
        self.enter(move |_| panic!("{why}"));
    }

    /// Each entry returns from the kernel by Power.
    fn enter_returning(&self) {
        let tree = self.tree.clone();
        self.enter(move |_| {
            tree.returned(false, false);
            Ok(())
        });
    }

    fn sleep(&self, f: impl FnMut(f64) -> Result<(), E> + 'static) {
        seams::SEAMS.with(|s| s.borrow_mut().sleep = Some(Box::new(f)));
    }

    fn control(&self, f: impl FnMut(&Root, &str, &str) -> Result<(), E> + 'static) {
        seams::SEAMS.with(|s| s.borrow_mut().control = Some(Box::new(f)));
    }

    fn run_sleep(&self) -> i64 {
        let requested = Stage {
            mode: Some("product"),
            ..Stage::new("requested", Some(0))
        };
        record(&self.root, requested).unwrap();
        execute(&self.root, self.lock.as_raw_fd()).unwrap()
    }

    fn receipt(&self) -> Value {
        self.fixture.json(RECEIPT)
    }

    fn product(&self) -> Value {
        self.receipt()["product"].clone()
    }

    fn guard(&self) -> Value {
        self.fixture.json(GUARD)
    }

    fn rpc(&self) -> Vec<String> {
        self.reborn.ops()
    }

    fn write(&self, name: &str, text: &str) {
        self.tree.write(name, text);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        seams::reset();
    }
}

fn refused(reason: &str) -> Result<(), E> {
    Err(E::Refused(reason.into()))
}

fn stages(receipt: &Value) -> Vec<String> {
    receipt["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["stage"].as_str().unwrap().to_owned())
        .collect()
}

fn index(stages: &[String], stage: &str) -> usize {
    stages
        .iter()
        .position(|s| s == stage)
        .unwrap_or_else(|| panic!("{stage} not in {stages:?}"))
}

/// `wakeup_count` writes as `pm_save_wakeup_count` takes them: a completed event since the read
/// invalidates the store even when no IRQ stays active. Every store is kept in `stores`.
fn kernel_control(
    stores: Rc<RefCell<Vec<String>>>,
) -> impl FnMut(&Root, &str, &str) -> Result<(), E> {
    move |root, name, value| {
        if name == "/sys/power/wakeup_count" {
            stores.borrow_mut().push(value.trim().to_owned());
            if Some(value.trim()) != root.read(name).as_deref() {
                return Err(os_error(libc::EINVAL, "intervening battery notification"));
            }
        }
        Ok(())
    }
}

#[test]
fn timer_maintenance_keeps_app_quiesced_then_power_fully_restores() {
    let p = Player::new();
    let entries = Rc::new(Cell::new(0));
    let (tree, ops, n) = (p.tree.clone(), p.reborn.ops.clone(), entries.clone());
    p.enter(move |_| {
        n.set(n.get() + 1);
        assert_eq!(*ops.lock().unwrap(), ["sleep_prepare"]);
        tree.returned(n.get() == 1, false);
        Ok(())
    });
    p.sleep(|_| Ok(()));
    assert_eq!(p.run_sleep(), 0);
    let receipt = p.receipt();
    assert_eq!(receipt["cycles"], 2);
    assert_eq!(receipt["maintenance_wakes"], 1);
    assert_eq!(receipt["suspended_seconds"].as_f64(), Some(625.0));
    assert!(
        receipt["first_entry"]["boottime_s"].as_f64().unwrap()
            < receipt["entry"]["boottime_s"].as_f64().unwrap()
    );
    assert_eq!(receipt["first_entry"]["voltage_uv"], 3900000);
    assert_eq!(receipt["product"]["wake_reason"], "power");
    assert_eq!(receipt["product"]["state"], "restored");
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
    assert_eq!(p.guard()["pending"], false);
    let requested = Stage {
        mode: Some("product"),
        ..Stage::new("requested", Some(0))
    };
    record(&p.root, requested).unwrap();
    assert!(p.receipt().get("first_entry").is_none());
}

#[test]
fn playback_may_start_while_the_radios_come_back() {
    // F24: a wired open right after a wake (shared activity lease) was refused as busy until the
    // radio restore had finished.
    let p = Player::new();
    p.write("/data/network/enabled", "1");
    let lease = Rc::new(RefCell::new(Vec::new()));
    let (seen, base) = (lease.clone(), p.fixture.base().to_owned());
    p.command(move |argv| {
        if argv.last() == Some(&"on-runtime") {
            let open = |name: &str| {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(base.join(name))
                    .unwrap()
            };
            let activity = open("run/y2/activity.lock");
            seen.borrow_mut()
                .push(if activity.try_lock_shared().is_ok() {
                    "free"
                } else {
                    "held"
                });
            // The firmware/update and maintenance locks stay held.
            let maintenance = open("data/system/platform/.maintenance.lock");
            seen.borrow_mut()
                .push(if maintenance.try_lock_shared().is_ok() {
                    "maintenance free"
                } else {
                    "maintenance held"
                });
        }
        done()
    });
    p.enter_returning();
    assert_eq!(p.run_sleep(), 0);
    assert_eq!(*lease.borrow(), ["free", "maintenance held"]);
}

#[test]
fn sleep_before_application_ready_waits_for_it_then_sleeps() {
    // 4fed1b6a (8.12): sleep 8 s after Reborn started, power features not yet armed, then a hang
    // right after resume. The kernel entry now waits.
    let p = Player::new();
    let boot = |stage: &str| json!({"boot_id": "same-boot", "last_stage": stage}).to_string();
    p.write("/data/system/platform/boot.json", &boot("services_started"));
    let ticks = Rc::new(Cell::new(0));
    let (t, tree) = (ticks.clone(), p.tree.clone());
    p.sleep(move |_| {
        t.set(t.get() + 1);
        if t.get() == 3 {
            tree.write(
                "/data/system/platform/boot.json",
                &json!({"boot_id": "same-boot", "last_stage": "application_ready"}).to_string(),
            );
        }
        Ok(())
    });
    let entered = Rc::new(RefCell::new(Vec::new()));
    let (e, t, tree) = (entered.clone(), ticks.clone(), p.tree.clone());
    p.enter(move |_| {
        e.borrow_mut().push(t.get());
        tree.returned(false, false);
        Ok(())
    });
    assert_eq!(p.run_sleep(), 0);
    assert_eq!(*entered.borrow(), [3]);
    let receipt = p.receipt();
    let stages = stages(&receipt);
    assert!(index(&stages, "reborn_prepared") < index(&stages, "waiting_application_ready"));
    assert!(index(&stages, "waiting_application_ready") < index(&stages, "quiescing_radios"));
    // Durable rows around the screen restore locate a later stall.
    assert!(index(&stages, "kernel_resumed") < index(&stages, "screen_restoring"));
    assert!(index(&stages, "screen_restoring") < index(&stages, "screen_restored"));
    assert!(index(&stages, "screen_restored") < index(&stages, "restoring_radios"));
    assert_eq!(receipt["product"]["state"], "restored");
}

#[test]
fn the_wait_for_application_ready_is_bounded_and_a_power_press_cancels_it() {
    let p = Player::new();
    p.write(
        "/data/system/platform/boot.json",
        &json!({"boot_id": "same-boot", "last_stage": "platform_start"}).to_string(),
    );
    let clock = Rc::new(Cell::new(1000.0));
    let c = clock.clone();
    p.sleep(move |seconds| {
        c.set(c.get() + seconds);
        Ok(())
    });
    let c = clock.clone();
    seams::SEAMS.with(|s| s.borrow_mut().monotonic = Some(Box::new(move || c.get())));
    p.enter_returning();
    assert_eq!(p.run_sleep(), 0);
    assert!(clock.get() - 1000.0 >= APPLICATION_READY_WAIT_S);
    assert_eq!(p.product()["state"], "restored");
    // Power during the wait: no kernel entry, the screen comes back.
    seams::SEAMS.with(|s| s.borrow_mut().monotonic = None);
    p.reborn.clear();
    p.write(
        "/sys/firmware/y2_pm/backstop_s",
        "provider=1 armed_s=0 running=0",
    );
    let tree = p.tree.clone();
    p.sleep(move |_| {
        tree.press();
        Ok(())
    });
    let entered = Rc::new(Cell::new(0));
    let e = entered.clone();
    p.enter(move |_| {
        e.set(e.get() + 1);
        Ok(())
    });
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(entered.get(), 0);
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
    let product = p.product();
    assert_eq!(
        (&product["state"], &product["reason"]),
        (&json!("refused"), &json!("wake_event_in_progress"))
    );
    assert_eq!(p.guard()["pending"], false);
}

#[test]
fn rtc_restores_without_automatic_reentry() {
    let p = Player::new();
    let tree = p.tree.clone();
    p.enter(move |_| {
        tree.returned(false, true);
        Ok(())
    });
    assert_eq!(p.run_sleep(), 0);
    let receipt = p.receipt();
    assert_eq!(receipt["cycles"], 1);
    assert_eq!(receipt["product"]["wake_reason"], "rtc");
}

#[test]
fn battery_notification_during_maintenance_uses_a_fresh_handshake() {
    let p = Player::new();
    let writes = Rc::new(RefCell::new(Vec::new()));
    p.control(kernel_control(writes.clone()));
    let entries = Rc::new(Cell::new(0));
    let (n, tree) = (entries.clone(), p.tree.clone());
    p.enter(move |_| {
        n.set(n.get() + 1);
        tree.returned(n.get() == 1, n.get() != 1);
        Ok(())
    });
    let tree = p.tree.clone();
    p.sleep(move |seconds| {
        assert_eq!(seconds, 2.0);
        // A routine post-resume BAT0 notification: no cable, Power or RTC event, and the normal
        // battery safety owner stays healthy.
        tree.write("/sys/power/wakeup_count", "9");
        tree.power();
        Ok(())
    });
    assert_eq!(p.run_sleep(), 0, "{}", p.product());
    assert_eq!(entries.get(), 2);
    assert_eq!(*writes.borrow(), ["8", "9"]);
    let receipt = p.receipt();
    assert_eq!(receipt["maintenance_wakes"], 1);
    assert_eq!(receipt["product"]["wake_reason"], "rtc");
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
}

#[test]
fn power_press_after_maintenance_read_still_cancels_entry() {
    let p = Player::new();
    let stores = Rc::new(RefCell::new(Vec::<String>::new()));
    let (s, tree) = (stores.clone(), p.tree.clone());
    p.control(move |root, name, value| {
        if name == "/sys/power/wakeup_count" {
            s.borrow_mut().push(value.trim().to_owned());
            if s.borrow().len() == 2 {
                tree.write(name, "10"); // Power pressed after the fresh read
                tree.press();
            }
            if Some(value.trim()) != root.read(name).as_deref() {
                return Err(os_error(libc::EINVAL, "new wakeup before entry"));
            }
        }
        Ok(())
    });
    let entries = Rc::new(Cell::new(0));
    let (n, tree) = (entries.clone(), p.tree.clone());
    p.enter(move |_| {
        n.set(n.get() + 1);
        tree.returned(true, false);
        Ok(())
    });
    p.sleep(|_| Ok(()));
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(entries.get(), 1);
    assert_eq!(stores.borrow().len(), 2);
    assert_eq!(p.product()["reason"], "wake_event_in_progress");
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
}

#[test]
fn battery_notification_during_preparation_takes_a_fresh_count() {
    // Archive 156, 10:31: Power to sleep; a BAT0 notification during the ≈6 s preparation
    // invalidated the first count and the sleep was refused.
    let p = Player::new();
    let stores = Rc::new(RefCell::new(Vec::new()));
    p.control(kernel_control(stores.clone()));
    let tree = p.tree.clone();
    p.reborn.handle(Box::new(move |op, dark| {
        if op == "sleep_prepare" {
            tree.write("/sys/power/wakeup_count", "9");
        }
        ready(op, dark)
    }));
    p.enter_returning();
    assert_eq!(p.run_sleep(), 0);
    assert_eq!(*stores.borrow(), ["8", "9"]);
    assert_eq!(p.product()["wake_reason"], "power");
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
}

#[test]
fn cable_during_preparation_cancels_instead_of_retrying() {
    let p = Player::new();
    p.control(|root, name, value| {
        if name == "/sys/power/wakeup_count" && Some(value.trim()) != root.read(name).as_deref() {
            return Err(os_error(libc::EINVAL, "cable event"));
        }
        Ok(())
    });
    let tree = p.tree.clone();
    p.reborn.handle(Box::new(move |op, dark| {
        if op == "sleep_prepare" {
            tree.write("/sys/power/wakeup_count", "9");
            tree.write("/sys/class/power_supply/y2-usb-presence/online", "1");
            tree.write("/sys/class/power_supply/BAT0/status", "Charging");
        }
        ready(op, dark)
    }));
    p.never_enter("slept with a cable");
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(p.product()["reason"], "charger_or_usb_active");
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore:dark"]);
}

#[test]
fn continuous_events_refuse_after_bounded_retries() {
    let p = Player::new();
    let stores = Rc::new(RefCell::new(Vec::new()));
    let (s, tree) = (stores.clone(), p.tree.clone());
    p.control(move |_, name, value| {
        if name == "/sys/power/wakeup_count" {
            s.borrow_mut().push(value.trim().to_owned());
            let count: u64 = value.trim().parse().unwrap();
            tree.write(name, &(count + 1).to_string()); // an event before every store
            return Err(os_error(libc::EINVAL, "busy wake source"));
        }
        Ok(())
    });
    p.never_enter("entered while busy");
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(stores.borrow().len(), STALE_COUNT_RETRIES as usize + 1);
    // A kernel wakeup source, not a person: the screen stays dark (F47).
    assert_eq!(p.product()["reason"], "wakeup_count_busy");
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore:dark"]);
}

#[test]
fn screen_woken_before_preparation_cancels_without_a_restore() {
    // F48: Power darkened the screen with the request; a second press woke it before the platform
    // prepared. Reborn refuses; nothing to restore.
    let p = Player::new();
    p.reborn
        .handle(Box::new(|_, _| refusal("wake_event_in_progress")));
    p.never_enter("slept after a wake");
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(p.product()["reason"], "wake_event_in_progress");
    assert_eq!(p.rpc(), ["sleep_prepare"]);
    assert_eq!(p.guard()["pending"], false);
    assert!(p.calls.borrow().is_empty()); // radios never quiesced
}

#[test]
fn kernel_busy_restores_once_without_claiming_sleep_or_retrying() {
    let p = Player::new();
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    p.enter(move |_| {
        c.set(c.get() + 1);
        Err(os_error(libc::EBUSY, "wake event during device suspend"))
    });
    assert_eq!(p.run_sleep(), 2);
    let receipt = p.receipt();
    assert_eq!(calls.get(), 1);
    assert_eq!(receipt["product"]["reason"], "kernel_suspend_refused");
    assert_eq!(receipt["product"]["kernel_completed"], false);
    assert!(receipt.get("cycles").is_none());
    // F47: the kernel refused (UART busy on a player); nobody woke it.
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore:dark"]);
    assert_eq!(receipt["reborn_restore"], true);
    assert_eq!(p.guard()["pending"], false);
}

#[test]
fn power_press_during_preparation_cancels_entry_and_restores() {
    let p = Player::new();
    let tree = p.tree.clone();
    p.reborn.handle(Box::new(move |op, dark| {
        if op == "sleep_prepare" {
            tree.press();
        }
        ready(op, dark)
    }));
    p.never_enter("lost Power gesture");
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
    assert_eq!(p.guard()["pending"], false);
}

#[test]
fn playback_preparation_refusal_remains_a_named_inhibitor() {
    let p = Player::new();
    p.reborn.handle(Box::new(|_, _| refusal("playback_active")));
    p.never_enter("entered during playback");
    assert_eq!(p.run_sleep(), 2);
    let value = p.product();
    assert_eq!(
        (&value["state"], &value["reason"]),
        (&json!("refused"), &json!("playback_active"))
    );
    assert_eq!(p.guard()["pending"], false);
}

#[test]
fn request_passes_lock_ownership_to_detached_helper() {
    let p = Player::new();
    p.lock.unlock().unwrap();
    let owned = Rc::new(RefCell::new(Vec::new()));
    let o = owned.clone();
    seams::SEAMS.with(|s| {
        s.borrow_mut().launch = Some(Box::new(move |_, lock| {
            o.borrow_mut().push(lock.try_clone().unwrap());
            Ok(())
        }))
    });
    let request =
        || -> Value { serde_json::from_str(&request::request(&p.root).unwrap().stdout).unwrap() };
    assert_eq!(request()["state"], "requested");
    assert_eq!(request()["reason"], "sleep_operation_in_progress");
    owned.borrow_mut().clear();
    assert_eq!(request()["state"], "requested");
    assert_eq!(owned.borrow().len(), 1);
}

#[test]
fn power_press_during_maintenance_gap_cancels_next_entry() {
    let p = Player::new();
    let calls = Rc::new(Cell::new(0));
    let (c, tree) = (calls.clone(), p.tree.clone());
    p.enter(move |_| {
        c.set(c.get() + 1);
        tree.returned(true, false);
        Ok(())
    });
    let tree = p.tree.clone();
    p.sleep(move |_| {
        tree.press();
        Ok(())
    });
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(calls.get(), 1);
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
    assert_eq!(p.guard()["pending"], false);
}

#[test]
fn rtc_during_maintenance_gap_cancels_next_entry() {
    let p = Player::new();
    let calls = Rc::new(Cell::new(0));
    let (c, tree) = (calls.clone(), p.tree.clone());
    p.enter(move |_| {
        c.set(c.get() + 1);
        tree.returned(true, false);
        Ok(())
    });
    let tree = p.tree.clone();
    p.sleep(move |_| {
        tree.alarm();
        tree.write("/sys/power/wakeup_count", "9");
        Ok(())
    });
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(calls.get(), 1);
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
    assert_eq!(p.guard()["pending"], false);
}

#[test]
fn active_owner_refuses_but_dead_owner_and_leftover_file_do_not() {
    let p = Player::new();
    // Another open file description holds the shared lease (Python: a child process); closing it
    // is the owner's end.
    let lease = p.fixture.path("/run/y2/activity.lock");
    let owner = File::create(&lease).unwrap();
    owner.lock_shared().unwrap();
    p.never_enter("entered with live owner");
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(p.product()["reason"], "activity_operation");
    drop(owner);
    assert!(lease.exists());
    p.enter_returning();
    assert_eq!(p.run_sleep(), 0);
}

#[test]
fn kernel_fault_quarantines_same_image_and_new_image_retries() {
    let p = Player::new();
    p.enter(|_| {
        Err(E::Os {
            class: "OSError",
            text: "failed after kernel entry".into(),
            errno: None,
        })
    });
    assert_eq!(p.run_sleep(), 1);
    assert_eq!(p.guard()["pending"], true);
    assert_eq!(
        p.root.read("/sys/firmware/y2_pm/backstop_s").as_deref(),
        Some("0")
    );
    // A regular fixture file records the write; real sysfs reads status.
    p.write(
        "/sys/firmware/y2_pm/backstop_s",
        "provider=1 armed_s=0 running=0",
    );
    assert_eq!(admission(&p.root), refused("sleep_recovery_required"));
    p.write(
        "/etc/y2linux/versions.json",
        r#"{"build_git_commit":"next-source"}"#,
    );
    assert_eq!(admission(&p.root), Ok(()));
}

#[test]
fn notifier_refusal_before_freeze_waits_instead_of_quarantining() {
    // c43e8711 (8.10): PM_SUSPEND_PREPARE refused with -EPERM before any task froze; the pending
    // guard then refused sleep 237 times in 4.4 h.
    let p = Player::new();
    let tree = p.tree.clone();
    p.enter(move |_| {
        tree.prepare_refusal_ring(&[]);
        Err(os_error(libc::EPERM, "Operation not permitted"))
    });
    assert_eq!(p.run_sleep(), 2);
    assert_eq!(p.product()["reason"], "kernel_prepare_refused");
    assert_eq!(p.guard()["pending"], false);
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore:dark"]);
    p.write(
        "/sys/firmware/y2_pm/backstop_s",
        "provider=1 armed_s=0 running=0",
    );
    assert_eq!(admission(&p.root), refused("kernel_prepare_retry_wait"));
    let retry = p.fixture.json(RETRY);
    let with = |key: &str, value: Value| {
        let mut changed: Map<String, Value> = retry.as_object().unwrap().clone();
        changed.insert(key.into(), value);
        p.write(RETRY, &Value::Object(changed).to_string());
    };
    let not_before = retry["not_before_ns"].as_u64().unwrap();
    with("not_before_ns", json!(not_before - 121 * 1_000_000_000));
    assert_eq!(admission(&p.root), Ok(()));
    // The wait belongs to one boot.
    with("boot_id", json!("earlier-boot"));
    assert_eq!(admission(&p.root), Ok(()));
}

#[test]
fn failure_after_freeze_or_unclear_ring_still_quarantines() {
    let p = Player::new();
    for (extra, ring) in [(&["TASKS_FROZEN"][..], true), (&[][..], false)] {
        let tree = p.tree.clone();
        p.enter(move |_| {
            if ring {
                tree.prepare_refusal_ring(extra);
            } else {
                tree.write(
                    "/sys/firmware/y2_pm/devices",
                    "ring=absent cycle=0 last=0 backstop_s=0",
                );
            }
            Err(os_error(libc::EPERM, "Operation not permitted"))
        });
        p.write(GUARD, "{}");
        p.write(
            "/sys/firmware/y2_pm/backstop_s",
            "provider=1 armed_s=0 running=0",
        );
        assert_eq!(p.run_sleep(), 1);
        assert_eq!(p.guard()["pending"], true);
        assert!(!p.fixture.exists(RETRY));
    }
}

#[test]
fn radio_quiesce_failure_before_kernel_entry_never_quarantines() {
    // POWER-FINAL-12 Defect A: a Bluetooth off refused by the reconnect owner (result 1, kernel
    // never entered) left the guard pending and every later sleep was refused, across reboots,
    // until a reflash.
    let p = Player::new();
    let calls = p.calls.clone();
    p.command(move |argv| {
        calls
            .borrow_mut()
            .push(argv.iter().map(|a| a.to_string()).collect());
        Outcome {
            ok: argv[1..] != ["bluetooth", "off-runtime"],
            ..done()
        }
    });
    p.sleep(|_| Ok(()));
    seams::SEAMS.with(|s| s.borrow_mut().radio_quiesce_seconds = Some(0.0));
    p.never_enter("entered without radios off");
    assert_eq!(p.run_sleep(), 1);
    assert_eq!(
        p.product()["reason"],
        "sleep_service_failed:y2-radio:bluetooth:attempts=1"
    );
    let guard = p.guard();
    assert_eq!(guard["pending"], false);
    assert_eq!(guard["kernel_entry"], false);
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
    assert_eq!(admission(&p.root), Ok(()));
    // The next request succeeds normally.
    seams::SEAMS.with(|s| s.borrow_mut().radio_quiesce_seconds = None);
    p.answer_commands();
    p.enter_returning();
    assert_eq!(p.run_sleep(), 0);
}

#[test]
fn bluetooth_operation_in_progress_is_waited_out_within_the_window() {
    let p = Player::new();
    let refusals = Rc::new(Cell::new(0));
    let r = refusals.clone();
    p.command(move |argv| {
        if argv[1..] == ["bluetooth", "off-runtime"] && r.get() < 3 {
            r.set(r.get() + 1);
            return Outcome {
                ok: false,
                reason: Some("command_failed".into()),
                output: Some(r#"{"ok": false}"#.into()),
            };
        }
        done()
    });
    let pauses = Rc::new(Cell::new(0));
    let c = pauses.clone();
    p.sleep(move |_| {
        c.set(c.get() + 1);
        Ok(())
    });
    p.enter_returning();
    assert_eq!(p.run_sleep(), 0);
    assert_eq!(refusals.get(), 3);
    assert_eq!(pauses.get(), 3);
    assert_eq!(p.product()["state"], "restored");
    assert_eq!(p.guard()["pending"], false);
}

#[test]
fn stale_pre_kernel_guard_from_any_boot_does_not_refuse() {
    let p = Player::new();
    let identity = identity(&p.root);
    for boot in ["same-boot", "earlier-boot"] {
        p.write(
            GUARD,
            &json!({"identity": identity, "pending": true, "kernel_entry": false, "boot_id": boot,
                    "updated": wall()})
            .to_string(),
        );
        assert_eq!(admission(&p.root), Ok(()));
    }
    // A pending kernel attempt (or a 12-format record without the field) still quarantines the
    // image.
    for kernel_entry in [Some(true), None] {
        let mut guard = json!({"identity": identity, "pending": true, "boot_id": "earlier-boot",
                               "updated": wall()});
        if let Some(entry) = kernel_entry {
            guard["kernel_entry"] = json!(entry);
        }
        p.write(GUARD, &guard.to_string());
        assert_eq!(admission(&p.root), refused("sleep_recovery_required"));
    }
}

#[test]
fn kernel_warn_alone_keeps_product_sleep() {
    // F26: one cfg80211 WARN (taint 512) kept a player awake on a weak charger for hours.
    let p = Player::new();
    p.write("/proc/sys/kernel/tainted", "512");
    assert_eq!(admission(&p.root), Ok(()));
    for value in ["4608", "1", "unreadable"] {
        // 4608: TAINT_WARN with TAINT_OOT_MODULE.
        p.write("/proc/sys/kernel/tainted", value);
        assert_eq!(
            admission(&p.root),
            refused("kernel_sleep_unavailable"),
            "taint {value}"
        );
    }
}

#[test]
fn old_kernel_and_tainted_diagnostic_cannot_enable_product_sleep() {
    let p = Player::new();
    p.write("/proc/sys/kernel/tainted", "4096");
    assert_eq!(admission(&p.root), refused("kernel_sleep_unavailable"));
    p.write(
        "/sys/bus/platform/devices/10006000.power-controller/state",
        "broken=0",
    );
    assert_eq!(
        admission(&p.root),
        refused("physical_qualification_required")
    );
}

#[test]
fn workload_expiry_and_mandatory_operation_exclusion() {
    let p = Player::new();
    p.write(
        "/sys/module/workload/parameters/leases",
        "Maintenance remaining_ms=3000 min_khz=598000",
    );
    assert_eq!(
        operation_state(&p.root, true),
        refused("workload_Maintenance")
    );
    p.write(
        "/sys/module/workload/parameters/leases",
        "Maintenance remaining_ms=0 min_khz=0",
    );
    assert_eq!(operation_state(&p.root, true), Ok(()));
    let held = operation_locks(&p.root).unwrap();
    assert!(matches!(
        Lease::take(&p.root, "Maintenance"),
        Err(Failure::Value(reason)) if reason == "sleep_operation_in_progress"
    ));
    drop(held);
    assert!(Lease::take(&p.root, "Maintenance").is_ok());
}

#[test]
fn pending_update_low_voltage_and_usb_each_prevent_entry() {
    let p = Player::new();
    for (path, value, reason) in [
        (
            "/sys/class/power_supply/BAT0/voltage_now",
            "3390000",
            "battery_reserve",
        ),
        (
            "/sys/class/power_supply/y2-usb-presence/online",
            "1",
            "charger_or_usb_active",
        ),
        (
            "/data/updates/state.json",
            r#"{"schema":1,"state":"Queued"}"#,
            "firmware_update",
        ),
    ] {
        let old = p.root.read(path);
        p.write(path, value);
        p.never_enter("unsafe entry");
        assert_eq!(p.run_sleep(), 2);
        assert_eq!(p.product()["reason"], reason);
        match old {
            None => fs::remove_file(p.fixture.path(path)).unwrap(),
            Some(old) => p.write(path, &old),
        }
    }
}

#[test]
fn low_battery_sleeps_until_the_low_battery_power_off() {
    // F55 (189640af, 8.41): 3.56-3.58 V under load at 37 % refused every sleep (recover_uv 3.6 V),
    // and the dark player drained awake.
    let p = Player::new();
    let power = |state: &str, battery: Value| {
        let mut battery = battery;
        if battery.get("confidence").is_none() {
            battery["confidence"] = json!("calibrated_estimate");
        }
        p.write(
            "/run/y2/power.json",
            &json!({"boot_id": "same-boot", "state": state,
                    "monotonic_ns": clock::monotonic_ns(), "battery": battery})
            .to_string(),
        );
    };
    p.write("/sys/class/power_supply/BAT0/voltage_now", "3558471");
    power(
        "Normal",
        json!({"percent": 37, "filtered_voltage_uv": 3578229}),
    );
    assert_eq!(battery_and_cable(&p.root), Ok(()));
    // Low (warning_soc) and Critical (critical_soc) sleep too: the 30-min maintenance return
    // admits again and wakes the player to power off.
    power(
        "Low",
        json!({"percent": 15, "filtered_voltage_uv": 3520000}),
    );
    assert_eq!(battery_and_cable(&p.root), Ok(()));
    power(
        "Critical",
        json!({"percent": 8, "filtered_voltage_uv": 3450000}),
    );
    assert_eq!(battery_and_cable(&p.root), Ok(()));
    // A dip under load is not empty; the filtered voltage decides.
    p.write("/sys/class/power_supply/BAT0/voltage_now", "3390000");
    assert_eq!(battery_and_cable(&p.root), Ok(()));
    for (state, battery) in [
        (
            "Critical",
            json!({"percent": 8, "filtered_voltage_uv": 3398000}),
        ),
        (
            "Critical",
            json!({"percent": 5, "filtered_voltage_uv": 3480000}),
        ),
        (
            "ShutdownPending",
            json!({"percent": 5, "filtered_voltage_uv": 3480000}),
        ),
    ] {
        power(state, battery.clone());
        let result = battery_and_cable(&p.root);
        assert!(
            result == refused("battery_reserve") || result == refused("battery_safety_owner"),
            "{state} {battery}: {result:?}"
        );
    }
    // Without a trusted percentage only the voltage floor applies.
    power(
        "Normal",
        json!({"percent": 5, "filtered_voltage_uv": 3700000, "confidence": "provisional"}),
    );
    assert_eq!(battery_and_cable(&p.root), Ok(()));
}

#[test]
fn idle_cable_without_host_admits_sleep() {
    // Defect C (archive 109): VBUS from a powered-off PC, charger idle.
    let p = Player::new();
    p.write("/sys/class/power_supply/y2-usb-presence/online", "1");
    p.write("/sys/class/udc/musb-hdrc.4.auto/state", "default");
    for status in ["Not charging", "Full"] {
        p.write("/sys/class/power_supply/BAT0/status", status);
        assert_eq!(battery_and_cable(&p.root), Ok(()));
    }
    // A configured host (SSH/MTP) or an active charge keeps the Y2 awake.
    p.write("/sys/class/udc/musb-hdrc.4.auto/state", "configured");
    assert_eq!(battery_and_cable(&p.root), refused("charger_or_usb_active"));
    p.write("/sys/class/udc/musb-hdrc.4.auto/state", "not attached");
    p.write("/sys/class/power_supply/BAT0/status", "Charging");
    assert_eq!(battery_and_cable(&p.root), refused("charger_or_usb_active"));
    // Without a cable the battery must really be discharging.
    p.write("/sys/class/power_supply/y2-usb-presence/online", "0");
    p.write("/sys/class/power_supply/BAT0/status", "Not charging");
    assert_eq!(
        battery_and_cable(&p.root),
        refused("battery_state_unavailable")
    );
}

// ---- the helper's part of tests/test_sleep_product.py -------------------------------------------

#[test]
fn screen_is_restored_before_the_radios() {
    // POWER-FINAL-17: archive 110 lit the screen ≈2.6 s after Power, 1.9 s of it waiting for
    // Wi-Fi/Bluetooth firmware and calibration.
    let p = Player::new();
    p.write("/data/network/enabled", "1");
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = events.clone();
    p.reborn.handle(Box::new(move |op, dark| {
        seen.lock().unwrap().push(format!("reborn {op}"));
        ready(op, dark)
    }));
    let seen = events.clone();
    p.command(move |argv| {
        seen.lock().unwrap().push(argv.join(" "));
        done()
    });
    p.enter_returning();
    assert_eq!(p.run_sleep(), 0);
    let events = events.lock().unwrap().clone();
    let at = |event: &str| events.iter().position(|e| e == event).unwrap();
    assert!(at("reborn sleep_restore") < at("/usr/sbin/y2-radio wifi on-runtime"));
    let receipt = p.receipt();
    let stages = stages(&receipt);
    assert!(index(&stages, "screen_restored") < index(&stages, "restoring_radios"));
    // The completion record reuses the early acknowledgement (restore_ack): one restore only.
    assert_eq!(p.rpc(), ["sleep_prepare", "sleep_restore"]);
    assert_eq!(receipt["reborn_restore"], true);
    assert_eq!(receipt["product"]["state"], "restored");
}

#[test]
fn early_screen_restore_is_reused_and_a_failed_one_retried() {
    let fixture = Fixture::new();
    for (name, text) in [
        ("/proc/sys/kernel/random/boot_id", "boot-one"),
        (
            "/sys/firmware/y2_pm/state",
            "valid=1 stage=EXIT failed_stage=NONE error=0 wake=0x20 \
             reset_resume_entry=0x59325253 pcm_length=596",
        ),
        ("/sys/firmware/y2_pm/stage", ""),
        ("/sys/power/state", ""),
        (
            "/sys/power/pm_test",
            "[none] core processors platform devices freezer",
        ),
    ] {
        fixture.put(name, text);
    }
    let root = &fixture.root;
    let reborn = Reborn::serve(
        &fixture,
        Box::new(|_, _| answer(json!({"sleep_ready": true, "screen_off": false}))),
    );
    let ack = screen_first(root, false).unwrap();
    assert_eq!(
        ack.clone().map(Value::Object),
        Some(json!({"sleep_ready": true, "screen_off": false}))
    );
    drop(reborn);
    assert_eq!(screen_first(root, false).unwrap(), None);
    let reborn = Reborn::serve(
        &fixture,
        Box::new(|_, _| answer(json!({"sleep_ready": true, "screen_off": false}))),
    );
    for stage in ["quiescing_radios", "kernel_suspend", "restoring_radios"] {
        record(root, Stage::new(stage, Some(0))).unwrap();
    }
    let complete = |restore_ack| Stage {
        restore_ack,
        ..Stage::new("complete_result_0", Some(0))
    };
    let value = record(root, complete(ack)).unwrap();
    assert!(reborn.ops().is_empty());
    assert_eq!(value["reborn_restore"], true);
    for stage in ["quiescing_radios", "kernel_suspend"] {
        record(root, Stage::new(stage, Some(0))).unwrap();
    }
    let value = record(root, complete(None)).unwrap();
    assert_eq!(reborn.ops().len(), 1);
    assert_eq!(value["reborn_restore"], true);
}

// ---- tests/test_pm_control_write.py ------------------------------------------------------------

/// `kernel_sleep` on a store whose first write(2) gives `result`; every write it was asked for.
fn invoke(result: Result<usize, i32>) -> (Result<(), E>, Vec<Vec<u8>>) {
    seams::reset();
    let fixture = Fixture::new();
    fixture.put("/sys/power/state", "");
    let calls = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
    let c = calls.clone();
    seams::SEAMS.with(|s| {
        s.borrow_mut().store = Some(Box::new(move |data| {
            c.borrow_mut().push(data.to_vec());
            // The first store can refuse; a repeated store would enter suspend.
            if c.borrow().len() == 1 {
                result
            } else {
                Ok(data.len())
            }
        }))
    });
    let outcome = kernel_sleep(&fixture.root);
    seams::reset();
    let calls = calls.borrow().clone();
    assert_eq!(calls, [b"mem\n".to_vec()], "PM command was replayed");
    (outcome, calls)
}

fn errno(result: Result<(), E>) -> Option<i32> {
    match result {
        Err(E::Os { errno, .. }) => errno,
        other => panic!("not an OSError: {other:?}"),
    }
}

#[test]
fn busy_and_failed_store_never_retries_during_close() {
    for code in [libc::EBUSY, libc::EAGAIN, libc::EIO] {
        let (outcome, _) = invoke(Err(code));
        assert_eq!(errno(outcome), Some(code));
    }
}

#[test]
fn short_store_fails_without_writing_remainder() {
    for size in [0, 2] {
        let (outcome, _) = invoke(Ok(size));
        assert_eq!(errno(outcome), Some(libc::EIO), "size {size}");
    }
}

#[test]
fn successful_store_is_issued_once() {
    let (outcome, _) = invoke(Ok(4));
    assert_eq!(outcome, Ok(()));
}
