//! `sleep.status` as a journal row holds it: the suspend receipt's product, read without promoting a
//! reset into a resume.
// SPDX-License-Identifier: GPL-2.0-only
use super::Dropped;
use y2p_core::{json::Object, py::PyValue};

/// `sleep.status(ctx)` on the receipt: durable evidence interpreted without promoting a reset into a
/// resume. `sleep status`'s product, except that a product that is set but no dict (a damaged
/// receipt, also a list of pairs `sleep status` would read) or that `sleep status` refuses drops the
/// row (counted; the daemon goes on).
pub(super) fn sleep_status(receipt: &Object, boot_id: Option<&str>) -> Result<Object, Dropped> {
    if receipt
        .get("product")
        .is_some_and(|p| p.truthy() && !p.is_object())
    {
        return Err(Dropped);
    }
    crate::commands::sleep::product(receipt, boot_id).map_err(|_| Dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use y2p_core::json::{dumps, ObjectExt};

    #[test]
    fn sleep_status_follows_sleep_py() {
        let parse = |t: &str| match serde_json::from_slice::<Value>(t.as_bytes()).unwrap() {
            Value::Object(o) => o,
            _ => unreachable!(),
        };
        let text = |o: &Object| dumps(o).unwrap();
        assert_eq!(
            text(&sleep_status(&parse("{}"), Some("b")).unwrap()),
            "{\"state\":\"idle\",\"reason\":null,\"request_boot_id\":null,\"kernel_completed\":false,\
             \"wake_reason\":\"unknown\",\"schema\":\"org.y2linux.sleep/v1\",\"boot_id\":\"b\",\"same_boot\":null}"
        );
        let other = parse(r#"{"product": {"state": "sleeping", "request_boot_id": "a"}}"#);
        let status = sleep_status(&other, Some("b")).unwrap();
        assert!(status.value("state").is_str("restore_failed"));
        assert!(sleep_status(&parse(r#"{"product": {"state": []}}"#), Some("b")).is_err());
        assert!(sleep_status(&parse(r#"{"product": 5}"#), Some("b")).is_err());
        assert!(sleep_status(&parse(r#"{"product": [1]}"#), Some("b")).is_err());
        // A list of pairs: dropped as well (the daemon goes on).
        assert!(sleep_status(&parse(r#"{"product": [["state", "idle"]]}"#), Some("b")).is_err());
    }
}
