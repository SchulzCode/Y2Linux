//! SIGTERM, SIGINT and SIGUSR1 as a readable pipe, so that the loop waits on them and on the
//! supplicant's events in one place; SIGUSR1 (usb.wake_service: re-read the media views now) only
//! wakes it.
// SPDX-License-Identifier: GPL-2.0-only
crate::signal_wake::signal_wake!();
