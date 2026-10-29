//! The `thermal` section: thermal zones with their trip points and cooling devices.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::util::{self, name, number, read};
use serde_json::{json, Value};
use y2p_core::Root;

/// `PurePath.stem` (Python 3.14: a trailing dot is a suffix too; a leading one is not).
fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

pub fn thermal(root: &Root) -> Value {
    let mut zones = Vec::new();
    for path in root.glob("/sys/class/thermal/thermal_zone*") {
        let mut trips = Vec::new();
        for trip in util::glob_dir(&path, "trip_point_*_temp") {
            let trip_name = name(&trip);
            trips.push(json!({"name": stem(&trip_name), "temperature_millicelsius": number(read(&trip).as_deref()),
                              "type": read(&trip.with_file_name(trip_name.replace("_temp", "_type")))}));
        }
        zones.push(
            json!({"name": name(&path), "type": read(&path.join("type")),
                          "temperature_millicelsius": number(read(&path.join("temp")).as_deref()),
                          "trips": trips}),
        );
    }
    let cooling: Vec<Value> = root
        .glob("/sys/class/thermal/cooling_device*")
        .iter()
        .map(|p| {
            json!({"name": name(p), "type": read(&p.join("type")),
                   "state": number(read(&p.join("cur_state")).as_deref()),
                   "max_state": number(read(&p.join("max_state")).as_deref())})
        })
        .collect();
    json!({"zones": zones, "cooling": cooling, "battery_temperature": null,
           "battery_temperature_reason": "no_qualified_pack_sensor"})
}
