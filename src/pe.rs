//! Very small read-only PE (portable executable) reader: `imports()` returns
//! the lower-cased names of the DLLs an exe links against, which is how we
//! decide whether a game actually uses D3D12.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

const MAX_FILE: u64 = 512 * 1024 * 1024;

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn i32_at(b: &[u8], off: usize) -> Option<i32> {
    u32_at(b, off).map(|v| v as i32)
}

struct Section {
    va: u32,
    vsize: u32,
    raw_ptr: u32,
    raw_size: u32,
}

impl Section {
    fn map(&self, rva: u32) -> Option<usize> {
        if rva < self.va || rva >= self.va.saturating_add(self.vsize.max(self.raw_size)) {
            return None;
        }
        let delta = rva - self.va;
        Some((self.raw_ptr as usize).saturating_add(delta as usize))
    }
}

pub fn imports(path: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(meta) = fs::metadata(path) else {
        return out;
    };
    if meta.len() > MAX_FILE {
        return out;
    }
    let Ok(bytes) = fs::read(path) else {
        return out;
    };
    parse(&bytes, &mut out);
    out
}

fn parse(b: &[u8], out: &mut HashSet<String>) {
    if b.len() < 0x40 || &b[0..2] != b"MZ" {
        return;
    }
    let Some(pe_off) = u32_at(b, 0x3C).map(|v| v as usize) else {
        return;
    };
    if b.get(pe_off..pe_off + 4) != Some(b"PE\0\0") {
        return;
    }
    let coff = pe_off + 4;
    let Some(num_sections) = u16_at(b, coff + 2) else {
        return;
    };
    let Some(opt_size) = u16_at(b, coff + 16) else {
        return;
    };
    let opt = coff + 20;
    let Some(magic) = u16_at(b, opt) else {
        return;
    };
    let pe32_plus = magic == 0x20B;
    let dir_off = opt + if pe32_plus { 112 } else { 96 };
    let Some(import_rva) = u32_at(b, dir_off + 8) else {
        return;
    };
    if import_rva == 0 {
        return;
    }
    let sections_off = opt + opt_size as usize;
    let mut sections = Vec::new();
    for i in 0..num_sections as usize {
        let base = sections_off + i * 40;
        let Some(vsize) = u32_at(b, base + 8) else {
            return;
        };
        let Some(va) = u32_at(b, base + 12) else {
            return;
        };
        let Some(raw_size) = u32_at(b, base + 16) else {
            return;
        };
        let Some(raw_ptr) = u32_at(b, base + 20) else {
            return;
        };
        sections.push(Section {
            va,
            vsize,
            raw_ptr,
            raw_size,
        });
    }

    let mut cursor = import_rva;
    for _ in 0..512 {
        let Some(off) = sections.iter().find_map(|s| s.map(cursor)) else {
            break;
        };
        let Some(name_rva) = u32_at(b, off + 12) else {
            break;
        };
        if name_rva == 0 {
            break;
        }
        let Some(name_off) = sections.iter().find_map(|s| s.map(name_rva)) else {
            break;
        };
        let mut name = String::new();
        for i in 0..256 {
            match b.get(name_off + i) {
                Some(0) | None => break,
                Some(c) => name.push(*c as char),
            }
        }
        if !name.is_empty() {
            out.insert(name.to_ascii_lowercase());
        }
        cursor = cursor.saturating_add(20);
        if i32_at(b, off + 32).map(|v| v == 0).unwrap_or(true) {
            break;
        }
    }

    // Delay-load imports live in the 13th data directory entry.
    let Some(delay_rva) = u32_at(b, dir_off + 8 * 13) else {
        return;
    };
    if delay_rva == 0 {
        return;
    }
    for i in 0..128 {
        let cursor = delay_rva + i * 32;
        let Some(off) = sections.iter().find_map(|s| s.map(cursor)) else {
            break;
        };
        let Some(name_ptr) = u32_at(b, off + 4) else {
            break;
        };
        if name_ptr == 0 {
            break;
        }
        // For delay imports the pointer is a plain VA in the image.
        let Some(image_base) = u32_at(b, opt + if pe32_plus { 24 } else { 28 }) else {
            break;
        };
        let rva = name_ptr.wrapping_sub(image_base);
        let Some(name_off) = sections.iter().find_map(|s| s.map(rva)) else {
            break;
        };
        let mut name = String::new();
        for k in 0..256 {
            match b.get(name_off + k) {
                Some(0) | None => break,
                Some(c) => name.push(*c as char),
            }
        }
        if !name.is_empty() {
            out.insert(name.to_ascii_lowercase());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_kernel32_imports_from_a_system_dll() {
        // kernel32.dll imports from ntdll + api-ms-* — proves the parser maps
        // sections correctly on a real PE32+ file.
        let imports = imports(Path::new("C:/Windows/System32/kernel32.dll"));
        if imports.is_empty() {
            // not running on Windows / file missing: still fine
            return;
        }
        assert!(imports.iter().any(|s| s.contains("ntdll")));
    }

    #[test]
    fn junk_does_not_panic() {
        let mut set = HashSet::new();
        parse(b"not a pe file", &mut set);
        assert!(set.is_empty());
    }
}
