// verify: debug ok
// verify: debug test
// verify: release ok
// A small ELF64 (little-endian) reader: header, program headers, section headers, .interp, and DT_NEEDED entries.
// It reads the running program's own file and cross-checks what it finds against what the kernel reports.

#[derive(Debug, PartialEq)]
enum ElfError {
    TooShort,
    BadMagic,
    Not64BitLittleEndian,
}

struct Elf<'a> {
    bytes: &'a [u8],
}

#[derive(Debug)]
struct Header {
    kind: u16,
    machine: u16,
    entry: u64,
    phoff: u64,
    shoff: u64,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

struct Segment {
    kind: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
}

struct Section {
    name: String,
    kind: u32,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
}

fn u16_at(b: &[u8], o: usize) -> u16 { u16::from_le_bytes(b[o..o + 2].try_into().unwrap()) }
fn u32_at(b: &[u8], o: usize) -> u32 { u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) }
fn u64_at(b: &[u8], o: usize) -> u64 { u64::from_le_bytes(b[o..o + 8].try_into().unwrap()) }

fn c_str(b: &[u8], o: usize) -> String {
    let end = b[o..].iter().position(|&c| c == 0).map_or(b.len(), |n| o + n);
    String::from_utf8_lossy(&b[o..end]).into_owned()
}

impl<'a> Elf<'a> {
    fn parse(bytes: &'a [u8]) -> Result<(Elf<'a>, Header), ElfError> {
        if bytes.len() < 64 {
            return Err(ElfError::TooShort);
        }
        if &bytes[0..4] != b"\x7fELF" {
            return Err(ElfError::BadMagic);
        }
        if bytes[4] != 2 || bytes[5] != 1 {
            return Err(ElfError::Not64BitLittleEndian); // EI_CLASS = ELFCLASS64, EI_DATA = ELFDATA2LSB
        }
        let h = Header {
            kind: u16_at(bytes, 16),
            machine: u16_at(bytes, 18),
            entry: u64_at(bytes, 24),
            phoff: u64_at(bytes, 32),
            shoff: u64_at(bytes, 40),
            phentsize: u16_at(bytes, 54),
            phnum: u16_at(bytes, 56),
            shentsize: u16_at(bytes, 58),
            shnum: u16_at(bytes, 60),
            shstrndx: u16_at(bytes, 62),
        };
        Ok((Elf { bytes }, h))
    }

    fn segments(&self, h: &Header) -> Vec<Segment> {
        (0..h.phnum as usize)
            .map(|i| {
                let o = h.phoff as usize + i * h.phentsize as usize;
                let b = self.bytes;
                Segment {
                    kind: u32_at(b, o),
                    flags: u32_at(b, o + 4),
                    offset: u64_at(b, o + 8),
                    vaddr: u64_at(b, o + 16),
                    filesz: u64_at(b, o + 32),
                    memsz: u64_at(b, o + 40),
                }
            })
            .collect()
    }

    fn sections(&self, h: &Header) -> Vec<Section> {
        let b = self.bytes;
        let raw: Vec<(u32, u32, u64, u64, u64, u32)> = (0..h.shnum as usize)
            .map(|i| {
                let o = h.shoff as usize + i * h.shentsize as usize;
                (u32_at(b, o), u32_at(b, o + 4), u64_at(b, o + 16), u64_at(b, o + 24), u64_at(b, o + 32), u32_at(b, o + 40))
            })
            .collect();
        let strtab_off = raw[h.shstrndx as usize].3 as usize;
        raw.iter()
            .map(|&(name, kind, addr, offset, size, link)| Section {
                name: c_str(b, strtab_off + name as usize),
                kind,
                addr,
                offset,
                size,
                link,
            })
            .collect()
    }

    /// DT_NEEDED entries: .dynamic holds (tag, value) pairs; for DT_NEEDED (1) the value is an offset into .dynstr.
    fn needed(&self, sections: &[Section]) -> Vec<String> {
        let Some(dynamic) = sections.iter().find(|s| s.name == ".dynamic") else { return vec![] };
        let dynstr = &sections[dynamic.link as usize];
        (0..dynamic.size as usize / 16)
            .map(|i| (u64_at(self.bytes, dynamic.offset as usize + i * 16), u64_at(self.bytes, dynamic.offset as usize + i * 16 + 8)))
            .take_while(|&(tag, _)| tag != 0)
            .filter(|&(tag, _)| tag == 1)
            .map(|(_, val)| c_str(self.bytes, dynstr.offset as usize + val as usize))
            .collect()
    }
}

fn segment_name(kind: u32) -> String {
    match kind {
        1 => "LOAD".into(), 2 => "DYNAMIC".into(), 3 => "INTERP".into(), 4 => "NOTE".into(), 6 => "PHDR".into(),
        7 => "TLS".into(), 0x6474e550 => "GNU_EH_FRAME".into(), 0x6474e551 => "GNU_STACK".into(),
        0x6474e552 => "GNU_RELRO".into(), 0x6474e553 => "GNU_PROPERTY".into(), k => format!("{k:#x}"),
    }
}

fn flags(f: u32) -> String {
    format!("{}{}{}", if f & 4 != 0 { 'R' } else { ' ' }, if f & 2 != 0 { 'W' } else { ' ' }, if f & 1 != 0 { 'E' } else { ' ' })
}

fn main() {
    let path = std::env::current_exe().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let (elf, h) = Elf::parse(&bytes).expect("an ELF64 file");
    println!("file {} bytes, type {} (3 = ET_DYN: PIE), machine {} (62 = x86-64)", bytes.len(), h.kind, h.machine);
    println!("entry {:#x}, {} program headers at {:#x}, {} section headers at {:#x}", h.entry, h.phnum, h.phoff, h.shnum, h.shoff);

    let segs = elf.segments(&h);
    println!("{:<13} {:>9} {:>9} {:>9} {:>9} flags", "segment", "offset", "vaddr", "filesz", "memsz");
    for s in &segs {
        println!("{:<13} {:>#9x} {:>#9x} {:>#9x} {:>#9x} {}", segment_name(s.kind), s.offset, s.vaddr, s.filesz, s.memsz, flags(s.flags));
    }

    let secs = elf.sections(&h);
    if let Some(interp) = secs.iter().find(|s| s.name == ".interp") {
        println!("interpreter: {}", c_str(&bytes, interp.offset as usize));
    }
    println!("needed: {:?}", elf.needed(&secs));
    let mut by_size: Vec<&Section> = secs.iter().filter(|s| s.kind != 8).collect(); // skip NOBITS (.bss, .tbss)
    by_size.sort_by_key(|s| std::cmp::Reverse(s.size));
    let top: Vec<String> = by_size.iter().take(6).map(|s| format!("{} {}", s.name, s.size)).collect();
    println!("largest sections: {}", top.join(", "));
    let alloc_bytes: u64 = secs.iter().filter(|s| s.addr != 0).map(|s| s.size).sum();
    println!("bytes in sections that get mapped (addr != 0): {alloc_bytes}; the rest is never loaded");

    // Cross-check with the kernel: it put the program headers at AT_PHDR and will start us at AT_ENTRY.
    // SAFETY: getauxval has no preconditions.
    let (at_phdr, at_entry) = unsafe { (libc::getauxval(libc::AT_PHDR), libc::getauxval(libc::AT_ENTRY)) };
    let phdr_vaddr = segs.iter().find(|s| s.kind == 6).map(|s| s.vaddr).unwrap();
    let load_base = at_phdr - phdr_vaddr;
    println!("load base {load_base:#x}; AT_ENTRY - load base = {:#x}; matches e_entry: {}", at_entry - load_base, at_entry - load_base == h.entry);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_header() -> Vec<u8> {
        let mut b = vec![0u8; 64];
        b[0..4].copy_from_slice(b"\x7fELF");
        b[4] = 2; // 64-bit
        b[5] = 1; // little-endian
        b[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        b[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        b[24..32].copy_from_slice(&0x1040u64.to_le_bytes()); // e_entry
        b
    }

    #[test]
    fn parses_header_fields() {
        let b = minimal_header();
        let (_, h) = Elf::parse(&b).unwrap();
        assert_eq!((h.kind, h.machine, h.entry, h.phnum), (3, 62, 0x1040, 0));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Elf::parse(&[0u8; 10]).err(), Some(ElfError::TooShort));
        let mut b = minimal_header();
        b[0] = b'M';
        assert_eq!(Elf::parse(&b).err(), Some(ElfError::BadMagic));
        let mut b = minimal_header();
        b[4] = 1; // 32-bit
        assert_eq!(Elf::parse(&b).err(), Some(ElfError::Not64BitLittleEndian));
    }
}
