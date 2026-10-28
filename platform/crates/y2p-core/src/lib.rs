//! Primitives every ported subcommand shares, each matching its Python counterpart in
//! `tools/platform/y2_platform/common.py` byte for byte (platform/CONTRACT.md, "Rules every subcommand
//! shares"): device paths below a root (`ctx.path`), bounded text reads (`ctx.read`), `json.dumps`
//! output, `atomic_json` records, flock and the kernel clocks.
// SPDX-License-Identifier: GPL-2.0-only
pub mod clock;
pub mod command;
pub mod connectivity_journal;
pub mod devices_support;
pub mod json;
pub mod lock;
pub mod py;
pub mod record;
pub mod root;
pub mod service_digest;
pub mod storage_media;

pub use root::Root;
