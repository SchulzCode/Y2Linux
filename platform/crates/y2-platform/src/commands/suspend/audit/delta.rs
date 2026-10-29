//! What `y2-power-audit`'s window measured: the deltas of two samples (threads, IRQs, CPU idle, OPP
//! residency, scheduler, wakeup sources), never a negative rate from a counter that was reset.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::{as_map, E};
use super::sample::{file, get, int, Sample, Thread};
use serde::Serialize;
use std::collections::BTreeMap;
use y2p_core::suspend_text::{split, splitlines};

pub(super) type Irqs = Vec<(String, (Vec<(String, i64)>, String))>;

pub(super) fn interrupts(sample: &Sample) -> Result<Irqs, E> {
    let text = file(sample, "/proc/interrupts");
    let lines = splitlines(text);
    let cpus: Vec<&str> = lines.first().map(|l| split(l)).unwrap_or_default();
    let mut rows: Irqs = Vec::new();
    for row in lines
        .iter()
        .skip(1)
        .filter_map(|line| y2p_core::text::interrupt_row(line))
    {
        let counts = row
            .counts
            .iter()
            .map(|c| int(c))
            .collect::<Result<Vec<_>, _>>()?;
        let columns = if counts.len() == cpus.len() {
            cpus.iter().map(|c| (*c).to_owned()).zip(counts).collect()
        } else {
            Vec::new()
        };
        let key = row.key.to_owned();
        let value = (columns, row.rest.join(" "));
        match rows.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => rows.push((key, value)),
        }
    }
    Ok(rows)
}

/// A row's CPU column names as a set.
pub(super) fn column_keys(list: &[(String, i64)]) -> Vec<&String> {
    let mut keys: Vec<&String> = list.iter().map(|(c, _)| c).collect();
    keys.sort();
    keys.dedup();
    keys
}

/// One thread's CPU and scheduling over the window.
#[derive(Clone, Serialize)]
pub(super) struct Offender {
    pub(super) thread: String,
    pub(super) name: String,
    pub(super) ticks: i64,
    pub(super) cpu_percent_one_core: f64,
    pub(super) context_switches_per_second: f64,
}

/// One IRQ line's count over the window. Offline CPU columns disappear, even though their earlier
/// IRQ totals remain relevant: a column change or a reset makes the delta unknown, and the
/// common-CPU evidence is kept apart.
#[derive(Serialize)]
pub(super) struct Irq {
    #[serde(skip)]
    pub(super) rank: i64,
    pub(super) irq: String,
    pub(super) name: String,
    pub(super) delta: Option<i64>,
    pub(super) per_second: Option<f64>,
    pub(super) counter_discontinuity: bool,
    pub(super) cpu_columns_changed: bool,
    #[serde(serialize_with = "as_map")]
    pub(super) common_cpu_deltas: Vec<(String, i64)>,
    pub(super) common_cpu_lower_bound_per_second: Option<f64>,
}

/// One CPU idle state's residency over the window.
#[derive(Serialize)]
pub(super) struct Idle {
    pub(super) path: String,
    pub(super) name: Option<String>,
    #[serde(rename = "raw_time_delta_us")]
    pub(super) raw: i64,
    pub(super) time_us: Option<i64>,
    #[serde(rename = "percent_wall_time")]
    pub(super) percent: Option<f64>,
    pub(super) entries: Option<i64>,
    #[serde(rename = "counter_discontinuity")]
    pub(super) discontinuity: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) residency_invalid_reason: Option<&'static str>,
}

/// One OPP's share of the recorded residency (keys sorted, as the record has always had them).
#[derive(Serialize)]
pub(super) struct Frequency {
    pub(super) khz: i64,
    pub(super) percent_recorded_residency: Option<f64>,
    pub(super) ticks: i64,
}

#[derive(Serialize)]
pub(super) struct Rate {
    pub(super) delta: i64,
    pub(super) per_second: f64,
}

#[derive(Serialize)]
pub(super) struct Wakeup {
    pub(super) name: String,
    #[serde(serialize_with = "as_map")]
    pub(super) deltas: Vec<(String, i64)>,
}

/// What the window measured.
#[derive(Serialize)]
pub(super) struct Summary {
    pub(super) top_cpu_threads: Vec<Offender>,
    pub(super) top_context_switch_threads: Vec<Offender>,
    pub(super) irqs: Vec<Irq>,
    pub(super) cpuidle: Vec<Idle>,
    pub(super) cpu0_idle_exits_per_second: Option<f64>,
    pub(super) frequency_residency: Vec<Frequency>,
    pub(super) frequency_reason: Option<&'static str>,
    #[serde(serialize_with = "as_map")]
    pub(super) scheduler: Vec<(String, Rate)>,
    pub(super) wakeup_sources: Vec<Wakeup>,
    pub(super) wakeups_per_second: Option<f64>,
    pub(super) wakeups_reason: &'static str,
}

pub(super) fn delta(a: &Sample, b: &Sample, duration: f64) -> Result<Summary, E> {
    if duration <= 0.0 || a.boot_id != b.boot_id {
        return Err(E::value("counter_window_crosses_boot_or_has_no_duration"));
    }
    // SAFETY: sysconf has no preconditions.
    let clock = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    let mut offenders = Vec::new();
    for (key, old) in &a.threads {
        let Some(new) = b.threads.iter().find(|(k, _)| k == key).map(|(_, t)| t) else {
            continue;
        };
        let ticks = new.utime + new.stime - old.utime - old.stime;
        let switches = |t: &Thread| t.switches.iter().map(|(_, v)| v).sum::<i64>();
        offenders.push(Offender {
            thread: key.clone(),
            name: new.name.clone(),
            ticks,
            cpu_percent_one_core: (100 * ticks) as f64 / clock / duration,
            context_switches_per_second: (switches(new) - switches(old)) as f64 / duration,
        });
    }
    let (ia, ib) = (interrupts(a)?, interrupts(b)?);
    let mut irqs: Vec<Irq> = Vec::new();
    for (key, (counts, name)) in &ib {
        let old: &[(String, i64)] = ia
            .iter()
            .find(|(k, _)| k == key)
            .map_or(&[], |(_, (c, _))| c.as_slice());
        let common: Vec<(String, i64)> = counts
            .iter()
            .filter_map(|(cpu, n)| {
                old.iter()
                    .find(|(c, _)| c == cpu)
                    .map(|(_, o)| (cpu.clone(), n - o))
            })
            .collect();
        let columns_changed = column_keys(old) != column_keys(counts);
        let reset = common.iter().any(|(_, v)| *v < 0);
        let valid = !common.is_empty() && !columns_changed && !reset;
        let sum: i64 = common.iter().map(|(_, v)| v).sum();
        let delta = valid.then_some(sum);
        // Subtracting column sums can silently undercount even when the apparent delta is
        // positive: the lower bound counts only the CPUs present at both ends.
        let lower = (!common.is_empty() && !reset).then_some(sum);
        irqs.push(Irq {
            rank: delta.unwrap_or(0),
            irq: key.clone(),
            name: name.clone(),
            delta,
            per_second: delta.map(|v| v as f64 / duration),
            counter_discontinuity: !valid,
            cpu_columns_changed: columns_changed,
            common_cpu_deltas: common,
            common_cpu_lower_bound_per_second: lower.map(|v| v as f64 / duration),
        });
    }
    let mut idle: Vec<Idle> = Vec::new();
    for (path, value) in &a.files {
        if !(path.contains("/cpuidle/") && path.ends_with("/time")) {
            continue;
        }
        let (Some(start), Some(Some(end))) = (value, get(&b.files, path)) else {
            continue;
        };
        let parent = path.rsplit_once('/').map_or("", |(p, _)| p).to_owned();
        let usage = format!("{parent}/usage");
        let time_us = int(end)? - int(start)?;
        let usage_of = |s: &Sample| -> Result<i64, E> {
            match get(&s.files, &usage) {
                Some(Some(text)) => int(text),
                Some(None) => Err(E::TypeError("int() argument must be a string".into())),
                None => Err(E::Invalid(format!(
                    "KeyError: {}",
                    y2p_core::py::repr(&usage)
                ))),
            }
        };
        let entries = usage_of(b)? - usage_of(a)?;
        let discontinuity = time_us < 0 || entries < 0;
        idle.push(Idle {
            name: get(&a.files, &format!("{parent}/name")).cloned().flatten(),
            path: parent,
            raw: time_us,
            time_us: (!discontinuity).then_some(time_us),
            percent: (!discontinuity).then(|| time_us as f64 / duration / 10000.0),
            entries: (!discontinuity).then_some(entries),
            discontinuity,
            residency_invalid_reason: None,
        });
    }
    // An idle interval is charged when that CPU exits it. An untouched secondary CPU can therefore
    // charge time that began before the first endpoint read. Reject impossible per-CPU totals
    // instead of publishing/clamping >100%.
    let cpu_of = |row: &Idle| {
        row.path
            .split("/cpuidle/")
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    let mut cpus: Vec<String> = Vec::new();
    for row in &idle {
        let cpu = cpu_of(row);
        if !cpus.contains(&cpu) {
            cpus.push(cpu);
        }
    }
    for cpu in cpus {
        let rows: Vec<usize> = (0..idle.len())
            .filter(|i| idle[*i].path.split("/cpuidle/").next() == Some(cpu.as_str()))
            .collect();
        if rows.iter().all(|i| idle[*i].time_us.is_some())
            && rows
                .iter()
                .map(|i| idle[*i].time_us.unwrap_or(0))
                .sum::<i64>() as f64
                > duration * 1e6
        {
            for i in rows {
                let row = &mut idle[i];
                row.time_us = None;
                row.percent = None;
                row.residency_invalid_reason = Some("idle_accounting_crosses_counter_window");
            }
        }
    }
    let opps = |s: &Sample| -> Result<Vec<(i64, i64)>, E> {
        let mut map: Vec<(i64, i64)> = Vec::new();
        for line in splitlines(file(
            s,
            "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state",
        )) {
            let parts = split(line);
            let [k, v] = parts.as_slice() else {
                return Err(E::value("not enough values to unpack"));
            };
            let (k, v) = (int(k)?, int(v)?);
            match map.iter_mut().find(|(key, _)| *key == k) {
                Some(entry) => entry.1 = v,
                None => map.push((k, v)),
            }
        }
        Ok(map)
    };
    let (fa, fb) = (opps(a)?, opps(b)?);
    let mut fd: BTreeMap<i64, i64> = BTreeMap::new();
    for (key, value) in &fb {
        if let Some((_, old)) = fa.iter().find(|(k, _)| k == key) {
            if value >= old {
                fd.insert(*key, value - old);
            }
        }
    }
    let total: i64 = fd.values().sum();
    let frequency: Vec<Frequency> = fd
        .iter()
        .map(|(&khz, &ticks)| Frequency {
            khz,
            percent_recorded_residency: (total != 0).then(|| (100 * ticks) as f64 / total as f64),
            ticks,
        })
        .collect();
    let proc_stat = |s: &Sample| -> Result<Vec<(String, i64)>, E> {
        let mut map: Vec<(String, i64)> = Vec::new();
        for line in splitlines(file(s, "/proc/stat")) {
            let parts = split(line);
            if let Some(&first) = parts.first() {
                if ["ctxt", "processes", "intr"].contains(&first) {
                    let value = int(parts
                        .get(1)
                        .ok_or_else(|| E::Invalid("IndexError".into()))?)?;
                    match map.iter_mut().find(|(k, _)| k == first) {
                        Some(entry) => entry.1 = value,
                        None => map.push((first.to_owned(), value)),
                    }
                }
            }
        }
        Ok(map)
    };
    let (sa, sb) = (proc_stat(a)?, proc_stat(b)?);
    let scheduler: Vec<(String, Rate)> = sa
        .iter()
        .filter_map(|(key, old)| {
            let new = sb.iter().find(|(k, _)| k == key)?.1;
            (new >= *old).then(|| {
                let rate = Rate {
                    delta: new - old,
                    per_second: (new - old) as f64 / duration,
                };
                (key.clone(), rate)
            })
        })
        .collect();
    let sources = |s: &Sample| -> Vec<(String, Vec<(String, String)>)> {
        let rows = splitlines(file(s, "/sys/kernel/debug/wakeup_sources"));
        let Some(first) = rows.first() else {
            return Vec::new();
        };
        let header = split(first);
        let mut result: Vec<(String, Vec<(String, String)>)> = Vec::new();
        for row in &rows[1..] {
            let parts = split(row);
            if let Some(&name) = parts.first() {
                let fields = header
                    .iter()
                    .skip(1)
                    .zip(parts.iter().skip(1))
                    .map(|(h, p)| ((*h).to_owned(), (*p).to_owned()))
                    .collect();
                match result.iter_mut().find(|(k, _)| k == name) {
                    Some(entry) => entry.1 = fields,
                    None => result.push((name.to_owned(), fields)),
                }
            }
        }
        result
    };
    let (wa, wb) = (sources(a), sources(b));
    let mut wakes = Vec::new();
    for (name, old) in &wa {
        let Some((_, new)) = wb.iter().find(|(k, _)| k == name) else {
            continue;
        };
        let mut deltas = Vec::new();
        for key in ["event_count", "wakeup_count", "active_count"] {
            let lookup = |list: &[(String, String)]| {
                list.iter()
                    .rev()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
            };
            if let (Some(o), Some(n)) = (lookup(old), lookup(new)) {
                deltas.push((key.to_owned(), int(&n)? - int(&o)?));
            }
        }
        wakes.push(Wakeup {
            name: name.clone(),
            deltas,
        });
    }
    let cpu0: Vec<&Idle> = idle.iter().filter(|r| r.path.contains("/cpu0/")).collect();
    let exits = (!cpu0.is_empty() && cpu0.iter().all(|r| r.entries.is_some()))
        .then(|| cpu0.iter().map(|r| r.entries.unwrap_or(0)).sum::<i64>());
    let mut by_ticks: Vec<&Offender> = offenders.iter().collect();
    by_ticks.sort_by_key(|x| std::cmp::Reverse(x.ticks));
    let mut by_rate: Vec<&Offender> = offenders.iter().collect();
    by_rate.sort_by(|x, y| {
        y.context_switches_per_second
            .partial_cmp(&x.context_switches_per_second)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    irqs.sort_by_key(|x| std::cmp::Reverse(x.rank));
    let frequency_reason = frequency
        .is_empty()
        .then_some("Kernel does not expose OPP residency counters.");
    Ok(Summary {
        top_cpu_threads: by_ticks.into_iter().take(25).cloned().collect(),
        top_context_switch_threads: by_rate.into_iter().take(25).cloned().collect(),
        irqs,
        cpuidle: idle,
        cpu0_idle_exits_per_second: exits.map(|e| e as f64 / duration),
        frequency_residency: frequency,
        frequency_reason,
        scheduler,
        wakeup_sources: wakes,
        wakeups_per_second: None,
        wakeups_reason:
            "Context switches and IRQs are proxies; they are not independent unique CPU wakes.",
    })
}
