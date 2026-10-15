//! The CPU profiles' encoding (Go's `profile.proto`, gzipped) and the
//! profiler's states, as tsgo's `CPUProfiler` reports them.

use super::*;

/// The top-level fields of an encoded message: (field, varint or bytes).
fn fields(mut bytes: &[u8]) -> Vec<(u64, Result<u64, Vec<u8>>)> {
    fn varint(bytes: &mut &[u8]) -> u64 {
        let mut value = 0;
        let mut shift = 0;
        loop {
            let byte = bytes[0];
            *bytes = &bytes[1..];
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return value;
            }
            shift += 7;
        }
    }
    let mut result = Vec::new();
    while !bytes.is_empty() {
        let key = varint(&mut bytes);
        let value = match key & 7 {
            0 => Ok(varint(&mut bytes)),
            2 => {
                let length = varint(&mut bytes) as usize;
                let (data, rest) = bytes.split_at(length);
                bytes = rest;
                Err(data.to_vec())
            }
            wire => panic!("unexpected wire type {wire}"),
        };
        result.push((key >> 3, value));
    }
    result
}

fn strings(profile: &[u8]) -> Vec<String> {
    fields(profile)
        .into_iter()
        .filter(|(field, _)| *field == 6)
        .map(|(_, value)| String::from_utf8(value.unwrap_err()).unwrap())
        .collect()
}

#[test]
fn crc32_is_gzips() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn a_gzip_member_inflates_to_its_data() {
    let data = b"perftools.profiles.Profile".repeat(20);
    let member = gzip(&data);
    assert_eq!(&member[..4], &[0x1f, 0x8b, 8, 0]);
    assert_eq!(crate::locale::gunzip(&member).as_deref(), Some(&data[..]));
}

#[test]
fn a_profile_has_gos_cpu_sample_types_and_period() {
    let frame = |name: &str, line| Frame {
        name: name.to_owned(),
        system_name: format!("_{name}"),
        file_name: "src/lib.rs".to_owned(),
        line,
    };
    let start = UNIX_EPOCH + Duration::from_secs(1);
    let profile = encode_profile(
        vec![
            (vec![frame("inner", 3), frame("outer", 9)], 2),
            (vec![frame("outer", 9)], 1),
        ]
        .into_iter(),
        start,
        Duration::from_millis(30),
    );
    let strings = strings(&profile);
    assert_eq!(
        strings[..5],
        ["", "samples", "count", "cpu", "nanoseconds"].map(String::from)
    );
    for name in ["inner", "_inner", "outer", "src/lib.rs"] {
        assert!(strings.iter().any(|string| string == name), "{name}");
    }
    let fields = fields(&profile);
    let count = |field| fields.iter().filter(|(number, _)| *number == field).count();
    // Two sample types, two samples, one location and function per name.
    assert_eq!((count(1), count(2), count(4), count(5)), (2, 2, 2, 2));
    let varint = |field| {
        fields
            .iter()
            .find(|(number, _)| *number == field)
            .map(|(_, value)| value.clone().unwrap())
    };
    assert_eq!(varint(9), Some(1_000_000_000));
    assert_eq!(varint(10), Some(30_000_000));
    assert_eq!(varint(12), Some(10_000_000));
    // The first sample counts twice: two samples, 20ms of CPU.
    let sample = fields
        .iter()
        .find(|(number, _)| *number == 2)
        .map(|(_, value)| value.clone().unwrap_err())
        .unwrap();
    let sample = self::fields(&sample);
    assert_eq!(sample[0].1.clone().unwrap_err(), vec![1, 2]);
    assert_eq!(
        sample[1].1.clone().unwrap_err(),
        vec![2, 0x80, 0xda, 0xc4, 0x09]
    );
}

#[test]
fn heap_and_allocation_profiles_are_not_supported() {
    assert_eq!(
        save_heap_profile("/tmp").unwrap_err(),
        "heap profiles are not supported by tsc-rs"
    );
    assert_eq!(
        save_alloc_profile("/tmp").unwrap_err(),
        "allocation profiles are not supported by tsc-rs"
    );
}

/// tsgo `CPUProfiler`: one profile at a time, written where it was asked
/// for when it stops.
#[cfg(unix)]
#[test]
fn a_cpu_profile_is_written_when_it_stops() {
    let _profiler = TEST_PROFILER.lock().unwrap_or_else(PoisonError::into_inner);
    let directory = std::env::temp_dir().join(format!("tsc-rs-pprof-test-{}", std::process::id()));
    let directory = directory.to_string_lossy().into_owned();
    let profiler = CpuProfiler::default();
    assert_eq!(
        profiler.stop().unwrap_err(),
        "CPU profiling not in progress"
    );
    profiler.start(&directory).unwrap();
    assert_eq!(
        profiler.start(&directory).unwrap_err(),
        "CPU profiling already in progress"
    );
    let mut value = 0u64;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(100) {
        value = std::hint::black_box(value.wrapping_mul(31).wrapping_add(7));
    }
    let path = profiler.stop().unwrap();
    assert!(path.starts_with(&directory), "{path}");
    assert!(path.ends_with("-cpuprofile.pb.gz"), "{path}");
    let profile = crate::locale::gunzip(&std::fs::read(&path).unwrap()).unwrap();
    assert!(strings(&profile).iter().any(|string| string == "samples"));
    assert_eq!(
        profiler.stop().unwrap_err(),
        "CPU profiling not in progress"
    );
    let _ = std::fs::remove_dir_all(&directory);
}
