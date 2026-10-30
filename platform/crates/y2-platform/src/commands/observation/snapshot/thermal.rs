//! The `thermal` section: thermal zones with their trip points and cooling devices.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::util::{self, int, name, read};
use serde::Serialize;
use std::path::Path;
use y2p_core::Root;

/// `PurePath.stem` (Python 3.14: a trailing dot is a suffix too; a leading one is not).
fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

/// One thermal zone with its trip points.
#[derive(Serialize)]
pub struct Zone {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub temperature_millicelsius: Option<i64>,
    pub trips: Vec<Trip>,
}

#[derive(Serialize)]
pub struct Trip {
    name: String,
    temperature_millicelsius: Option<i64>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Serialize)]
pub struct Cooling {
    name: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    state: Option<i64>,
    max_state: Option<i64>,
}

/// The `thermal` section (in this key order).
#[derive(Serialize)]
pub struct Thermal {
    pub zones: Vec<Zone>,
    pub cooling: Vec<Cooling>,
    /// No qualified pack sensor: always null.
    battery_temperature: Option<i64>,
    battery_temperature_reason: &'static str,
}

/// A sysfs integer (`int(text)`), none when unreadable or no integer.
fn integer(path: &Path) -> Option<i64> {
    read(path).as_deref().and_then(int)
}

pub fn thermal(root: &Root) -> Thermal {
    let zones = root
        .glob("/sys/class/thermal/thermal_zone*")
        .iter()
        .map(|path| Zone {
            name: name(path),
            kind: read(&path.join("type")),
            temperature_millicelsius: integer(&path.join("temp")),
            trips: util::glob_dir(path, "trip_point_*_temp")
                .iter()
                .map(|trip| {
                    let trip_name = name(trip);
                    Trip {
                        name: stem(&trip_name).to_owned(),
                        temperature_millicelsius: integer(trip),
                        kind: read(&trip.with_file_name(trip_name.replace("_temp", "_type"))),
                    }
                })
                .collect(),
        })
        .collect();
    let cooling = root
        .glob("/sys/class/thermal/cooling_device*")
        .iter()
        .map(|p| Cooling {
            name: name(p),
            kind: read(&p.join("type")),
            state: integer(&p.join("cur_state")),
            max_state: integer(&p.join("max_state")),
        })
        .collect();
    Thermal {
        zones,
        cooling,
        battery_temperature: None,
        battery_temperature_reason: "no_qualified_pack_sensor",
    }
}
