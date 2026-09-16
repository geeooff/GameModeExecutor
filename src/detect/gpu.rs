//! Per-process GPU utilisation, read from the counters Task Manager shows.
//!
//! This is not a detector and never decides whether a game is running. It
//! answers a narrower question: among the processes Windows' known game list
//! already matched, which one is actually rendering? A launcher stub and an
//! anti-cheat service sit near zero on the 3D engine; the game does not.
//!
//! Intel's PresentMon measures the same thing properly, by tracing frame
//! presentation through ETW, but that needs administrator rights or membership
//! of *Performance Log Users*. These counters need neither on most accounts,
//! and every caller here treats a failure to read them as "no opinion".

use std::collections::HashMap;

use anyhow::{Result, bail};
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW,
    PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};
use windows::core::PCWSTR;

/// Wildcard over every process and every engine of every adapter.
const COUNTER_PATH: &str = r"\GPU Engine(*)\Utilization Percentage";

const PDH_MORE_DATA: u32 = 0x8000_07D2;

/// Engines that mean "this process is drawing". Video decode and copy engines
/// are busy for a video player or a download too, so they are left out.
const RENDERING_ENGINES: &[&str] = &["3d", "vr", "compute"];

/// A PDH query, closed on drop.
struct Query(PDH_HQUERY);

impl Drop for Query {
    fn drop(&mut self) {
        // SAFETY: the handle came from `PdhOpenQueryW` and is closed once.
        unsafe { PdhCloseQuery(self.0) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Rendering load per process id, as a percentage summed over the engines of
/// every adapter. Values can exceed 100 on a multi-adapter or multi-engine
/// machine, which is fine: only their order matters here.
///
/// Utilisation is a rate, so it needs two samples. `interval` is how long to
/// wait between them; a second is what Task Manager uses.
pub fn rendering_load(interval: std::time::Duration) -> Result<HashMap<u32, f64>> {
    let mut handle = PDH_HQUERY::default();
    // SAFETY: a null data source means the live machine; `handle` is a valid
    // out pointer, and the query it receives is owned by `Query` below.
    let status = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut handle) };
    if status != 0 {
        bail!("PdhOpenQueryW failed (0x{status:08X})");
    }
    let query = Query(handle);

    let path = wide(COUNTER_PATH);
    let mut counter = PDH_HCOUNTER::default();
    // SAFETY: `path` is NUL-terminated and outlives the call; the query is
    // open, and the counter lives and dies with it.
    let status = unsafe { PdhAddEnglishCounterW(query.0, PCWSTR(path.as_ptr()), 0, &mut counter) };
    if status != 0 {
        bail!("cannot add the GPU Engine counter (0x{status:08X})");
    }

    // A rate needs a baseline and a second reading.
    // SAFETY: the query is open for as long as `query` lives.
    let status = unsafe { PdhCollectQueryData(query.0) };
    if status != 0 {
        bail!("first PdhCollectQueryData failed (0x{status:08X})");
    }
    std::thread::sleep(interval);
    // SAFETY: as above.
    let status = unsafe { PdhCollectQueryData(query.0) };
    if status != 0 {
        bail!("second PdhCollectQueryData failed (0x{status:08X})");
    }

    collect(counter)
}

fn collect(counter: PDH_HCOUNTER) -> Result<HashMap<u32, f64>> {
    let mut bytes = 0u32;
    let mut items = 0u32;

    // First call sizes the buffer and is expected to fail with PDH_MORE_DATA.
    // SAFETY: with no buffer the API only writes the two sizes.
    let status = unsafe {
        PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut bytes, &mut items, None)
    };
    if status != PDH_MORE_DATA {
        bail!("sizing the counter array failed (0x{status:08X})");
    }
    if items == 0 {
        return Ok(HashMap::new());
    }

    // Allocated as items rather than bytes so the buffer is aligned for them.
    // PDH writes the instance name strings into the tail of the same block.
    let size = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    let mut buffer: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> =
        Vec::with_capacity(bytes as usize / size + 1);
    // SAFETY: the capacity is at least `bytes` bytes, which is what `bytes`
    // tells the API it may write, so the items and the strings behind them
    // all land inside the allocation.
    let status = unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut bytes,
            &mut items,
            Some(buffer.as_mut_ptr()),
        )
    };
    if status != 0 {
        bail!("reading the counter array failed (0x{status:08X})");
    }
    // SAFETY: the API initialised exactly `items` structs at the front of the
    // buffer. The strings it wrote after them stay inside the capacity, so the
    // `szName` pointers remain valid until `buffer` is dropped -- which is
    // after the loop below.
    unsafe { buffer.set_len(items as usize) };

    let mut load: HashMap<u32, f64> = HashMap::new();
    for item in &buffer {
        // SAFETY: `szName` points into `buffer`'s tail, still allocated and
        // NUL-terminated by PDH.
        let name = unsafe { item.szName.to_string() }.unwrap_or_default();
        let Some((pid, engine)) = parse_instance(&name) else {
            continue;
        };
        if !RENDERING_ENGINES.contains(&engine.as_str()) {
            continue;
        }
        // SAFETY: the array was requested as PDH_FMT_DOUBLE, so this is the
        // union member PDH wrote.
        let value = unsafe { item.FmtValue.Anonymous.doubleValue };
        if value.is_finite() && value > 0.0 {
            *load.entry(pid).or_default() += value;
        }
    }
    Ok(load)
}

/// Instance names look like
/// `pid_12345_luid_0x00000000_0x0001A2B3_phys_0_eng_0_engtype_3D`.
fn parse_instance(name: &str) -> Option<(u32, String)> {
    let lowered = name.to_ascii_lowercase();
    let pid = lowered
        .strip_prefix("pid_")?
        .split('_')
        .next()?
        .parse::<u32>()
        .ok()?;
    let engine = lowered.rsplit_once("engtype_")?.1.to_owned();
    Some((pid, engine))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_names_are_parsed() {
        assert_eq!(
            parse_instance("pid_12345_luid_0x00000000_0x0001A2B3_phys_0_eng_0_engtype_3D"),
            Some((12345, "3d".to_owned()))
        );
        assert_eq!(
            parse_instance("pid_7_luid_0x0_0x1_phys_0_eng_2_engtype_VideoDecode"),
            Some((7, "videodecode".to_owned()))
        );
    }

    #[test]
    fn anything_else_is_ignored() {
        assert_eq!(parse_instance("engtype_3D"), None);
        assert_eq!(parse_instance("pid_notanumber_engtype_3D"), None);
        assert_eq!(parse_instance(""), None);
    }

    #[test]
    fn only_rendering_engines_count() {
        assert!(RENDERING_ENGINES.contains(&"3d"));
        // A video player would otherwise outrank a paused game.
        assert!(!RENDERING_ENGINES.contains(&"videodecode"));
        assert!(!RENDERING_ENGINES.contains(&"copy"));
    }

    #[test]
    fn reading_the_counters_never_panics() {
        // Not an assertion about every account: reading these can be refused,
        // and every caller treats that as "no opinion" rather than an error.
        match rendering_load(std::time::Duration::from_millis(200)) {
            Ok(load) => println!("read {} process(es) with rendering load", load.len()),
            Err(error) => println!("counters unavailable here: {error:#}"),
        }
    }
}
