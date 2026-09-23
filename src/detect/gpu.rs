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
//!
//! The counters are read through the PerfLib V2 consumer functions, not PDH.
//! *GPU Engine* is a V2 counter set, registered by the display kernel, and
//! Microsoft offers these functions "when you need to collect V2 countersets
//! with minimal dependencies and overhead". PDH read the same values, but
//! kept what it loaded to resolve the counter's path for the rest of the
//! process's life: some 3.7 MB, left by the first read. Through PerfLib the
//! same read leaves 0.2 MB. `docs/design/16-footprint.md` has the
//! measurements.
//!
//! The counter set and its counter are found by their English names, as
//! Microsoft's consumer guide says to do without the provider's symbol
//! file, and the counter's registered type is checked, since the formula
//! depends on it. What PerfLib hands back is a documented byte layout,
//! parsed here in plain code with every size bounded, so the parsing is
//! tested without Windows.
//! <https://learn.microsoft.com/en-us/windows/win32/perfctrs/using-the-perflib-functions-to-consume-counter-data>

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{ERROR_NOT_ENOUGH_MEMORY, HANDLE};
use windows::Win32::System::Performance::{
    PERF_COUNTER_IDENTIFIER, PERF_COUNTER_RATE, PERF_DATA_HEADER, PERF_DELTA_COUNTER,
    PERF_DISPLAY_PERCENT, PERF_ERROR_RETURN, PERF_MULTIPLE_INSTANCES,
    PERF_REG_COUNTER_ENGLISH_NAMES, PERF_REG_COUNTERSET_ENGLISH_NAME, PERF_REG_COUNTERSET_STRUCT,
    PERF_SIZE_LARGE, PERF_TIMER_100NS, PERF_TYPE_COUNTER, PerfAddCounters, PerfCloseQueryHandle,
    PerfEnumerateCounterSet, PerfOpenQueryHandle, PerfQueryCounterData,
    PerfQueryCounterSetRegistrationInfo, PerfRegInfoType,
};
use windows::core::GUID;

/// The counter set, and the counter in it, by their English names.
const COUNTER_SET: &str = "GPU Engine";
const COUNTER: &str = "Utilization Percentage";

/// `PERF_100NSEC_TIMER`, as `winperf.h` composes it: a percentage of the
/// elapsed time, computed as `100 * (N1 - N0) / (D1 - D0)` over two samples,
/// with `D` the sample's time in 100 ns units.
/// <https://learn.microsoft.com/en-us/windows/win32/perfctrs/calculating-counter-values>
const PERF_100NSEC_TIMER: u32 = PERF_SIZE_LARGE
    | PERF_TYPE_COUNTER
    | PERF_COUNTER_RATE
    | PERF_TIMER_100NS
    | PERF_DELTA_COUNTER
    | PERF_DISPLAY_PERCENT;

/// Engines that mean "this process is drawing". Video decode and copy engines
/// are busy for a video player or a download too, so they are left out.
const RENDERING_ENGINES: &[&str] = &["3d", "vr", "compute"];

/// Rendering load per process id, as a percentage summed over the engines of
/// every adapter. Values can exceed 100 on a multi-adapter or multi-engine
/// machine, which is fine: only their order matters here.
///
/// Utilisation is a rate, so it needs two samples. `interval` is how long to
/// wait between them; a second is what Task Manager uses.
pub fn rendering_load(interval: std::time::Duration) -> Result<HashMap<u32, f64>> {
    let (set, counter) = locate()?;
    let query = Query::open(&set, counter)?;
    let first = query.sample()?;
    std::thread::sleep(interval);
    let second = query.sample()?;
    Ok(load_between(
        &parse_sample(&first)?,
        &parse_sample(&second)?,
    ))
}

/// The counter set's identifier and the counter's, found by name, with the
/// counter's type checked against the formula `load_between` applies.
fn locate() -> Result<(GUID, u32)> {
    let set = counter_sets()?
        .into_iter()
        .find(|set| {
            registration(set, PERF_REG_COUNTERSET_ENGLISH_NAME)
                .is_ok_and(|name| utf16_at(&name, 0).as_deref() == Some(COUNTER_SET))
        })
        .with_context(|| format!("no `{COUNTER_SET}` counter set on this machine"))?;
    let names = registration(&set, PERF_REG_COUNTER_ENGLISH_NAMES)?;
    let counter = counter_names(&names)
        .into_iter()
        .find_map(|(id, name)| (name == COUNTER).then_some(id))
        .with_context(|| format!("no `{COUNTER}` counter in `{COUNTER_SET}`"))?;
    let structure = registration(&set, PERF_REG_COUNTERSET_STRUCT)?;
    match counter_type(&structure, counter) {
        Some(PERF_100NSEC_TIMER) => Ok((set, counter)),
        other => bail!("`{COUNTER}` has type {other:X?}, not the timer it was"),
    }
}

/// Every counter set registered on this machine.
fn counter_sets() -> Result<Vec<GUID>> {
    let mut count = 0u32;
    let mut sets = Vec::new();
    loop {
        // SAFETY: the slice carries its own length, which bounds the write;
        // `count` is a local.
        let status = unsafe { PerfEnumerateCounterSet(None, Some(&mut sets), &mut count) };
        match status {
            0 => {
                sets.truncate(count as usize);
                return Ok(sets);
            }
            _ if status == ERROR_NOT_ENOUGH_MEMORY.0 => {
                sets = vec![GUID::zeroed(); count as usize];
            }
            _ => bail!("PerfEnumerateCounterSet failed ({status})"),
        }
    }
}

/// One piece of a counter set's registration, sized then read.
fn registration(set: &GUID, code: PerfRegInfoType) -> Result<Vec<u8>> {
    let mut size = 0u32;
    let mut buffer = Vec::new();
    loop {
        // SAFETY: `set` outlives the call; the slice carries its own length,
        // which bounds the write; `size` is a local.
        let status = unsafe {
            PerfQueryCounterSetRegistrationInfo(None, set, code, 0, Some(&mut buffer), &mut size)
        };
        match status {
            0 => {
                buffer.truncate(size as usize);
                return Ok(buffer);
            }
            _ if status == ERROR_NOT_ENOUGH_MEMORY.0 => buffer = vec![0; size as usize],
            _ => bail!("PerfQueryCounterSetRegistrationInfo failed ({status})"),
        }
    }
}

/// A PerfLib query handle, closed on drop.
struct Query(HANDLE);

impl Drop for Query {
    fn drop(&mut self) {
        // SAFETY: the handle came from `PerfOpenQueryHandle` and is closed once.
        unsafe { PerfCloseQueryHandle(self.0) };
    }
}

/// What `PerfAddCounters` takes for a multi-instance counter set: the
/// identifier, then the instance name -- `*`, every instance -- padded to a
/// multiple of eight bytes, as in Microsoft's own sample.
#[repr(C)]
struct Specification {
    identifier: PERF_COUNTER_IDENTIFIER,
    instance: [u16; 4],
}

impl Query {
    fn open(set: &GUID, counter: u32) -> Result<Self> {
        let mut handle = HANDLE::default();
        // SAFETY: a null machine is this one; `handle` is a valid out pointer,
        // and the query it receives is owned by `Query` below.
        let status = unsafe { PerfOpenQueryHandle(None, &mut handle) };
        if status != 0 {
            bail!("PerfOpenQueryHandle failed ({status})");
        }
        let query = Self(handle);
        let mut specification = Specification {
            identifier: PERF_COUNTER_IDENTIFIER {
                CounterSetGuid: *set,
                Size: size_of::<Specification>() as u32,
                CounterId: counter,
                // Every instance id: the name filter is what selects.
                InstanceId: u32::MAX,
                ..Default::default()
            },
            instance: [u16::from(b'*'), 0, 0, 0],
        };
        let size = specification.identifier.Size;
        // SAFETY: the pointer is to the whole block, whose first field is the
        // identifier (`repr(C)`); the block is `size` bytes long, as the call
        // is told, and lives on this frame for the whole call.
        let status = unsafe {
            PerfAddCounters(
                query.0,
                (&raw mut specification).cast::<PERF_COUNTER_IDENTIFIER>(),
                size,
            )
        };
        // The call can succeed while refusing the one specification it was
        // given; that answer is in the block itself.
        let refused = specification.identifier.Status;
        if status != 0 || refused != 0 {
            bail!("PerfAddCounters failed ({status}, {refused})");
        }
        Ok(query)
    }

    /// One sample, as the bytes PerfLib wrote.
    fn sample(&self) -> Result<Vec<u8>> {
        let mut size = 0u32;
        // Eight-byte words, so the header's 64-bit fields are aligned.
        let mut words: Vec<u64> = Vec::new();
        loop {
            // SAFETY: the buffer is `len * 8` bytes, eight-aligned, and the
            // call is told exactly that; `size` is a local.
            let status = unsafe {
                PerfQueryCounterData(
                    self.0,
                    Some(words.as_mut_ptr().cast::<PERF_DATA_HEADER>()),
                    (words.len() * 8) as u32,
                    &mut size,
                )
            };
            match status {
                0 => break,
                _ if status == ERROR_NOT_ENOUGH_MEMORY.0 => {
                    words = vec![0; (size as usize).div_ceil(8)];
                }
                _ => bail!("PerfQueryCounterData failed ({status})"),
            }
        }
        Ok(words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .take(size as usize)
            .collect())
    }
}

// ------------------------------------------------ the documented layouts --

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// A NUL-terminated UTF-16LE string starting at `at`; `None` when the NUL
/// is missing, which a well-formed block never does.
fn utf16_at(bytes: &[u8], at: usize) -> Option<String> {
    let mut units = Vec::new();
    for pair in bytes.get(at..)?.as_chunks::<2>().0 {
        match u16::from_le_bytes(*pair) {
            0 => return Some(String::from_utf16_lossy(&units)),
            unit => units.push(unit),
        }
    }
    None
}

/// The counters' names: a `PERF_STRING_BUFFER_HEADER` -- its size and a
/// count -- then one `PERF_STRING_COUNTER_HEADER` per counter, an id and
/// the offset of its name from the start of the block.
fn counter_names(block: &[u8]) -> Vec<(u32, String)> {
    let count = u32_at(block, 4).unwrap_or(0) as usize;
    (0..count)
        .map_while(|i| {
            let at = 8 + i * 8;
            Some((u32_at(block, at)?, u32_at(block, at + 4)?))
        })
        .filter_map(|(id, offset)| Some((id, utf16_at(block, offset as usize)?)))
        .collect()
}

/// A counter's type: a `PERF_COUNTERSET_REG_INFO` of 32 bytes -- its
/// fourth field the number of counters -- then that many
/// `PERF_COUNTER_REG_INFO` of 48 bytes, id first and type second.
fn counter_type(block: &[u8], counter: u32) -> Option<u32> {
    const SET: usize = 32;
    const COUNTER: usize = 48;
    let count = u32_at(block, 24)? as usize;
    (0..count)
        .map_while(|i| {
            let at = SET + i * COUNTER;
            Some((u32_at(block, at)?, u32_at(block, at + 4)?))
        })
        .find_map(|(id, kind)| (id == counter).then_some(kind))
}

/// One sample of a single counter over every instance: when, and each
/// instance's raw value by name and id.
#[derive(Debug, PartialEq)]
struct Sample {
    /// `PerfTime100NSec`, the time base of a `PERF_100NSEC_TIMER`.
    time: i64,
    values: HashMap<(String, u32), u64>,
}

/// A `PERF_DATA_HEADER` of 48 bytes, then one `PERF_COUNTER_HEADER` of 16
/// -- status, type, size -- of type `PERF_MULTIPLE_INSTANCES`: a
/// `PERF_MULTI_INSTANCES` of 8 bytes, its second field the count, then per
/// instance a `PERF_INSTANCE_HEADER` -- its size, its id, its name, padded
/// -- and a `PERF_COUNTER_DATA` -- the value's size, the block's, the value.
fn parse_sample(bytes: &[u8]) -> Result<Sample> {
    const DATA_HEADER: usize = 48;
    const COUNTER_HEADER: usize = 16;
    let bad = || anyhow::anyhow!("the counter data is not laid out as documented");

    let total = u32_at(bytes, 0).ok_or_else(bad)? as usize;
    let bytes = bytes.get(..total).ok_or_else(bad)?;
    if u32_at(bytes, 4) != Some(1) {
        bail!("one counter was asked for, not {:?}", u32_at(bytes, 4));
    }
    let time = u64_at(bytes, 16).ok_or_else(bad)? as i64;

    let status = u32_at(bytes, DATA_HEADER).ok_or_else(bad)?;
    let kind = u32_at(bytes, DATA_HEADER + 4).ok_or_else(bad)?;
    if kind == PERF_ERROR_RETURN.0 as u32 {
        bail!("the provider answered with error {status}");
    }
    if kind != PERF_MULTIPLE_INSTANCES.0 as u32 {
        bail!("unexpected result type {kind}");
    }
    let end = DATA_HEADER + u32_at(bytes, DATA_HEADER + 8).ok_or_else(bad)? as usize;
    let block = bytes.get(..end).ok_or_else(bad)?;

    let instances = DATA_HEADER + COUNTER_HEADER;
    let count = u32_at(block, instances + 4).ok_or_else(bad)?;
    let mut at = instances + 8;
    let mut values = HashMap::new();
    for _ in 0..count {
        // Each size covers its own header, so neither can be under eight
        // bytes, and every step moves forward.
        let header = u32_at(block, at).ok_or_else(bad)? as usize;
        if header < 8 {
            return Err(bad());
        }
        let id = u32_at(block, at + 4).ok_or_else(bad)?;
        let data = at.checked_add(header).ok_or_else(bad)?;
        // The name ends inside its own block, or it is not a name.
        let name = block
            .get(..data)
            .and_then(|instance| utf16_at(instance, at + 8))
            .ok_or_else(bad)?;
        let value_size = u32_at(block, data).ok_or_else(bad)?;
        let data_size = u32_at(block, data + 4).ok_or_else(bad)? as usize;
        if data_size < 8 {
            return Err(bad());
        }
        let value = match value_size {
            8 => u64_at(block, data + 8),
            4 => u32_at(block, data + 8).map(u64::from),
            _ => None,
        }
        .ok_or_else(bad)?;
        values.insert((name, id), value);
        at = data.checked_add(data_size).ok_or_else(bad)?;
    }
    Ok(Sample { time, values })
}

/// The rendering load per process between two samples. An instance that
/// is not in both, or whose value went backwards -- an engine that came or
/// went, which Microsoft's example drops too -- says nothing.
fn load_between(first: &Sample, second: &Sample) -> HashMap<u32, f64> {
    let elapsed = second.time - first.time;
    let mut load: HashMap<u32, f64> = HashMap::new();
    if elapsed <= 0 {
        return load;
    }
    for (key, &value) in &second.values {
        let Some((pid, engine)) = parse_instance(&key.0) else {
            continue;
        };
        if !RENDERING_ENGINES.contains(&engine.as_str()) {
            continue;
        }
        let Some(busy) = first
            .values
            .get(key)
            .and_then(|&before| value.checked_sub(before))
        else {
            continue;
        };
        let share = 100.0 * busy as f64 / elapsed as f64;
        if share > 0.0 {
            *load.entry(pid).or_default() += share;
        }
    }
    load
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

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn pad(bytes: &mut Vec<u8>) {
        while !bytes.len().is_multiple_of(8) {
            bytes.push(0);
        }
    }

    /// A sample laid out as PerfLib lays one out: `time`, then each
    /// instance's name, id and 8-byte value.
    fn sample_bytes(time: i64, instances: &[(&str, u32, u64)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, id, value) in instances {
            let mut header = vec![0; 8];
            header.extend(utf16(name));
            pad(&mut header);
            let size = header.len() as u32;
            header[0..4].copy_from_slice(&size.to_le_bytes());
            header[4..8].copy_from_slice(&id.to_le_bytes());
            body.extend(header);
            body.extend(8u32.to_le_bytes());
            body.extend(16u32.to_le_bytes());
            body.extend(value.to_le_bytes());
        }
        let mut instances_block = Vec::new();
        instances_block.extend((8 + body.len() as u32).to_le_bytes());
        instances_block.extend((instances.len() as u32).to_le_bytes());
        instances_block.extend(body);

        let mut counter = Vec::new();
        counter.extend(0u32.to_le_bytes());
        counter.extend((PERF_MULTIPLE_INSTANCES.0 as u32).to_le_bytes());
        counter.extend((16 + instances_block.len() as u32).to_le_bytes());
        counter.extend(0u32.to_le_bytes());
        counter.extend(instances_block);

        let mut bytes = Vec::new();
        bytes.extend((48 + counter.len() as u32).to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(0i64.to_le_bytes()); // PerfTimeStamp
        bytes.extend(time.to_le_bytes()); // PerfTime100NSec
        bytes.extend(10_000_000i64.to_le_bytes()); // PerfFreq
        bytes.extend([0; 16]); // SystemTime
        bytes.extend(counter);
        bytes
    }

    const GAME_3D: &str = "pid_4242_luid_0x00000000_0x0001A2B3_phys_0_eng_0_engtype_3D";
    const GAME_COPY: &str = "pid_4242_luid_0x00000000_0x0001A2B3_phys_0_eng_5_engtype_Copy";
    const LAUNCHER_3D: &str = "pid_77_luid_0x00000000_0x0001A2B3_phys_0_eng_0_engtype_3D";

    #[test]
    fn a_sample_is_read_as_laid_out() {
        let bytes = sample_bytes(5_000, &[(GAME_3D, 3, 1_000), (LAUNCHER_3D, 0, 20)]);
        let sample = parse_sample(&bytes).unwrap();
        assert_eq!(sample.time, 5_000);
        assert_eq!(sample.values.len(), 2);
        assert_eq!(sample.values[&(GAME_3D.to_owned(), 3)], 1_000);
        assert_eq!(sample.values[&(LAUNCHER_3D.to_owned(), 0)], 20);
    }

    #[test]
    fn an_empty_sample_is_empty() {
        let sample = parse_sample(&sample_bytes(1, &[])).unwrap();
        assert!(sample.values.is_empty());
    }

    #[test]
    fn a_cut_sample_is_refused_not_misread() {
        let bytes = sample_bytes(5_000, &[(GAME_3D, 3, 1_000), (LAUNCHER_3D, 0, 20)]);
        for cut in [0, 10, 48, 60, 72, 100, bytes.len() - 1] {
            // The header's own total says more than there is.
            assert!(parse_sample(&bytes[..cut]).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn a_count_larger_than_the_block_is_refused() {
        let mut bytes = sample_bytes(5_000, &[(GAME_3D, 3, 1_000)]);
        // PERF_MULTI_INSTANCES' count, just after the two headers.
        bytes[68..72].copy_from_slice(&9u32.to_le_bytes());
        assert!(parse_sample(&bytes).is_err());
    }

    #[test]
    fn an_error_from_the_provider_is_an_error() {
        let mut bytes = sample_bytes(5_000, &[(GAME_3D, 3, 1_000)]);
        bytes[52..56].copy_from_slice(&(PERF_ERROR_RETURN.0 as u32).to_le_bytes());
        assert!(parse_sample(&bytes).is_err());
    }

    #[test]
    fn load_is_the_busy_share_of_the_interval() {
        // One second in 100 ns units; the game's 3D engine busy 600 ms of it,
        // the launcher's 5 ms, the game's copy engine all of it.
        let first = parse_sample(&sample_bytes(
            0,
            &[(GAME_3D, 0, 0), (GAME_COPY, 0, 0), (LAUNCHER_3D, 0, 100)],
        ))
        .unwrap();
        let second = parse_sample(&sample_bytes(
            10_000_000,
            &[
                (GAME_3D, 0, 6_000_000),
                (GAME_COPY, 0, 10_000_000),
                (LAUNCHER_3D, 0, 50_100),
            ],
        ))
        .unwrap();
        let load = load_between(&first, &second);
        assert!((load[&4242] - 60.0).abs() < 1e-9, "copy engine left out");
        assert!((load[&77] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn an_engine_that_came_or_went_says_nothing() {
        let first = parse_sample(&sample_bytes(0, &[(GAME_3D, 0, 500)])).unwrap();
        let second = parse_sample(&sample_bytes(
            10_000_000,
            &[(GAME_3D, 0, 100), (LAUNCHER_3D, 0, 9_000)],
        ))
        .unwrap();
        // The game's value went backwards, the launcher was not there before.
        assert!(load_between(&first, &second).is_empty());
    }

    #[test]
    fn no_time_elapsed_is_no_load() {
        let sample = parse_sample(&sample_bytes(7, &[(GAME_3D, 0, 500)])).unwrap();
        assert!(load_between(&sample, &sample).is_empty());
    }

    #[test]
    fn counter_names_are_read_from_their_offsets() {
        let mut block = vec![0; 8 + 2 * 8];
        let first = block.len() as u32;
        block.extend(utf16("Running Time"));
        let second = block.len() as u32;
        block.extend(utf16("Utilization Percentage"));
        block[4..8].copy_from_slice(&2u32.to_le_bytes());
        for (i, (id, offset)) in [(1u32, first), (2, second)].into_iter().enumerate() {
            block[8 + i * 8..12 + i * 8].copy_from_slice(&id.to_le_bytes());
            block[12 + i * 8..16 + i * 8].copy_from_slice(&offset.to_le_bytes());
        }
        assert_eq!(
            counter_names(&block),
            vec![
                (1, "Running Time".to_owned()),
                (2, "Utilization Percentage".to_owned())
            ]
        );
        // A name whose offset points outside the block is left out.
        block[12..16].copy_from_slice(&9_999u32.to_le_bytes());
        assert_eq!(counter_names(&block).len(), 1);
    }

    #[test]
    fn a_counter_type_is_found_by_id() {
        let mut block = vec![0; 32 + 2 * 48];
        block[24..28].copy_from_slice(&2u32.to_le_bytes());
        for (i, (id, kind)) in [(1u32, 0x0001_0100u32), (2, PERF_100NSEC_TIMER)]
            .into_iter()
            .enumerate()
        {
            let at = 32 + i * 48;
            block[at..at + 4].copy_from_slice(&id.to_le_bytes());
            block[at + 4..at + 8].copy_from_slice(&kind.to_le_bytes());
        }
        assert_eq!(counter_type(&block, 2), Some(PERF_100NSEC_TIMER));
        assert_eq!(counter_type(&block, 1), Some(0x0001_0100));
        assert_eq!(counter_type(&block, 3), None);
        assert_eq!(counter_type(&block[..40], 2), None);
    }

    #[test]
    fn the_timer_type_is_the_one_registered() {
        // What `GPU Engine\Utilization Percentage` declares, read on
        // 2026-09-23: 0x20510500, `PERF_100NSEC_TIMER` in winperf.h.
        assert_eq!(PERF_100NSEC_TIMER, 0x2051_0500);
    }

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
