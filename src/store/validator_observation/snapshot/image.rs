//! Conservative Linux linkage proof; foreign or unknown images are unsupported.
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;

/// Hashing the host is sound only when extraction and validation execute from
/// that same file. Match executable mappings by device/inode, never pathname.
#[inline(never)]
pub(super) fn open_executable_with_code(code: &[usize]) -> Option<File> {
    let executable = File::open("/proc/self/exe").ok()?;
    let metadata = executable.metadata().ok()?;
    if !metadata.is_file() || metadata.ino() == 0 {
        return None;
    }
    let mut maps = String::new();
    File::open("/proc/self/maps")
        .ok()?
        .take(1_048_577)
        .read_to_string(&mut maps)
        .ok()?;
    if maps.len() > 1_048_576 {
        return None;
    }
    let mut addresses = code.iter().copied().chain(std::iter::once(
        open_executable_with_code as *const () as usize,
    ));
    // Linux's dev_t layout (the same split used by major()/minor()).
    let device = metadata.dev();
    let major = ((device >> 8) & 0xfff) | ((device >> 32) & 0xfffff000);
    let minor = (device & 0xff) | ((device >> 12) & 0xffffff00);
    if addresses.all(|address| mapping_contains(&maps, address, metadata.ino(), major, minor)) {
        Some(executable)
    } else {
        None
    }
}

fn mapping_contains(maps: &str, address: usize, inode: u64, major: u64, minor: u64) -> bool {
    maps.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let Some((start, end)) = fields.next().and_then(|range| range.split_once('-')) else {
            return false;
        };
        let Some(permissions) = fields.next() else {
            return false;
        };
        let Some(_offset) = fields.next().and_then(|s| u64::from_str_radix(s, 16).ok()) else {
            return false;
        };
        let Some((device_major, device_minor)) = fields.next().and_then(|dev| dev.split_once(':'))
        else {
            return false;
        };
        let mapped_inode = fields.next().and_then(|s| s.parse::<u64>().ok());
        let range = usize::from_str_radix(start, 16)
            .ok()
            .zip(usize::from_str_radix(end, 16).ok());
        permissions.len() == 4
            && matches!(permissions.as_bytes()[0], b'r' | b'-')
            && matches!(permissions.as_bytes()[1], b'w' | b'-')
            && permissions.as_bytes()[2] == b'x'
            && matches!(permissions.as_bytes()[3], b'p' | b's')
            && mapped_inode == Some(inode)
            && u64::from_str_radix(device_major, 16).ok() == Some(major)
            && u64::from_str_radix(device_minor, 16).ok() == Some(minor)
            && range.is_some_and(|(start, end)| start <= address && address < end)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_requires_executable_range_and_exact_device_inode() {
        let maps = "1000-2000 r-xp 00000000 08:01 42 /path with spaces (deleted)\n";
        assert!(mapping_contains(maps, 0x1000, 42, 8, 1));
        assert!(!mapping_contains(maps, 0x2000, 42, 8, 1));
        assert!(!mapping_contains(maps, 0x1000, 41, 8, 1));
        assert!(!mapping_contains(maps, 0x1000, 42, 8, 2));
        assert!(!mapping_contains(maps, 0x1000, 42, 9, 1));
        assert!(!mapping_contains(
            &maps.replace("r-xp", "rw-p"),
            0x1000,
            42,
            8,
            1
        ));
        assert!(!mapping_contains("malformed", 0x1000, 42, 8, 1));
        assert!(!mapping_contains(
            &maps.replace("00000000", "not-hex"),
            0x1000,
            42,
            8,
            1
        ));
        assert!(!mapping_contains("", 0x1000, 42, 8, 1));
    }

    #[test]
    fn ordinary_executable_is_supported_but_unmapped_code_is_not() {
        assert!(open_executable_with_code(&[]).is_some());
        assert!(open_executable_with_code(&[0]).is_none());
    }
}
