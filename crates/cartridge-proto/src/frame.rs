use std::ffi::CString;
use std::io;
use std::sync::atomic::{fence, AtomicU64, Ordering};

pub const MAX_W: u32 = 2048;
pub const MAX_H: u32 = 2048;

const MAGIC: u64 = 0xCA27_F4A3_0000_0001;
const HEADER: usize = 64;
const SLOT_HEADER: usize = 32;
const SLOT: usize = SLOT_HEADER + (MAX_W as usize) * (MAX_H as usize) * 4;
const SIZE: usize = HEADER + 2 * SLOT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrcFormat {
    Xrgb8888,
    Rgb565,
    Rgb1555,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
    pub aspect: f32,
    pub seq: u64,
}

pub fn convert_row(src: &[u8], dst: &mut [u8], format: SrcFormat) {
    match format {
        SrcFormat::Xrgb8888 => {
            for (s, d) in src
                .as_chunks::<4>()
                .0
                .iter()
                .zip(dst.as_chunks_mut::<4>().0)
            {
                *d = [s[2], s[1], s[0], 0xff];
            }
        }
        SrcFormat::Rgb565 => {
            for (s, d) in src
                .as_chunks::<2>()
                .0
                .iter()
                .zip(dst.as_chunks_mut::<4>().0)
            {
                let p = u16::from_le_bytes(*s);
                let g = ((p >> 5) & 0x3f) as u32 * 255 / 63;
                *d = [expand5(p >> 11), g as u8, expand5(p), 0xff];
            }
        }
        SrcFormat::Rgb1555 => {
            for (s, d) in src
                .as_chunks::<2>()
                .0
                .iter()
                .zip(dst.as_chunks_mut::<4>().0)
            {
                let p = u16::from_le_bytes(*s);
                *d = [expand5(p >> 10), expand5(p >> 5), expand5(p), 0xff];
            }
        }
    }
}

fn expand5(v: u16) -> u8 {
    ((v & 0x1f) as u32 * 255 / 31) as u8
}

struct Shm {
    ptr: *mut u8,
}

// SAFETY: the mapping is shared memory; concurrent access is coordinated through the atomics.
unsafe impl Send for Shm {}

impl Shm {
    fn open(name: &CString, create: bool) -> io::Result<Self> {
        let flags = if create {
            libc::O_CREAT | libc::O_EXCL | libc::O_RDWR
        } else {
            libc::O_RDWR
        };
        let fd = unsafe { libc::shm_open(name.as_ptr(), flags, 0o600 as libc::c_uint) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mapped = Self::map(fd, create);
        unsafe { libc::close(fd) };
        if mapped.is_err() && create {
            unsafe { libc::shm_unlink(name.as_ptr()) };
        }
        mapped
    }

    fn map(fd: libc::c_int, create: bool) -> io::Result<Self> {
        if create && unsafe { libc::ftruncate(fd, SIZE as libc::off_t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut st) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if (st.st_size as usize) < SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame buffer too small",
            ));
        }
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { ptr: ptr.cast() })
    }

    fn atomic(&self, offset: usize) -> &AtomicU64 {
        // SAFETY: offsets are 8-aligned within the mapping, which lives as long as self.
        unsafe { &*(self.ptr.add(offset) as *const AtomicU64) }
    }

    fn seq(&self) -> &AtomicU64 {
        self.atomic(8)
    }

    fn get_u32(&self, offset: usize) -> u32 {
        unsafe { std::ptr::read_volatile(self.ptr.add(offset) as *const u32) }
    }

    fn set_u32(&self, offset: usize, value: u32) {
        unsafe { std::ptr::write_volatile(self.ptr.add(offset) as *mut u32, value) }
    }

    fn slot(seq: u64) -> usize {
        HEADER + (seq % 2) as usize * SLOT
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr.cast(), SIZE) };
    }
}

pub struct FrameReader {
    shm: Shm,
    name: CString,
}

impl FrameReader {
    pub fn create(name: &str) -> io::Result<Self> {
        let name = CString::new(name)?;
        let shm = Shm::open(&name, true)?;
        shm.atomic(0).store(MAGIC, Ordering::Release);
        Ok(Self { shm, name })
    }

    pub fn read_into(&self, last_seq: u64, out: &mut Vec<u8>) -> Option<FrameInfo> {
        let seq = self.shm.seq().load(Ordering::Acquire);
        if seq == 0 || seq == last_seq {
            return None;
        }
        let base = Shm::slot(seq);
        let generation = self.shm.atomic(base);
        let before = generation.load(Ordering::Acquire);
        if before % 2 == 1 {
            return None;
        }
        let width = self.shm.get_u32(base + 8).min(MAX_W);
        let height = self.shm.get_u32(base + 12).min(MAX_H);
        let aspect = f32::from_bits(self.shm.get_u32(base + 16));
        let len = width as usize * height as usize * 4;
        out.resize(len, 0);
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.shm.ptr.add(base + SLOT_HEADER),
                out.as_mut_ptr(),
                len,
            )
        };
        fence(Ordering::Acquire);
        if generation.load(Ordering::Relaxed) != before || len == 0 {
            return None;
        }
        Some(FrameInfo {
            width,
            height,
            aspect,
            seq,
        })
    }
}

impl Drop for FrameReader {
    fn drop(&mut self) {
        unsafe { libc::shm_unlink(self.name.as_ptr()) };
    }
}

pub struct FrameWriter {
    shm: Shm,
}

impl FrameWriter {
    pub fn open(name: &str) -> io::Result<Self> {
        let shm = Shm::open(&CString::new(name)?, false)?;
        if shm.atomic(0).load(Ordering::Acquire) != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a frame buffer",
            ));
        }
        Ok(Self { shm })
    }

    pub fn write(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
        pitch: usize,
        format: SrcFormat,
        aspect: f32,
    ) {
        let w = width.min(MAX_W) as usize;
        let h = height.min(MAX_H) as usize;
        let bpp = if format == SrcFormat::Xrgb8888 { 4 } else { 2 };
        if w == 0 || h == 0 || pitch < w * bpp || data.len() < (h - 1) * pitch + w * bpp {
            return;
        }
        let next = self.shm.seq().load(Ordering::Relaxed) + 1;
        let base = Shm::slot(next);
        let generation = self.shm.atomic(base);
        let g = generation.load(Ordering::Relaxed);
        generation.store(g + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        // SAFETY: the slot's pixel area holds MAX_W * MAX_H * 4 bytes and w, h are clamped.
        let pixels = unsafe {
            std::slice::from_raw_parts_mut(self.shm.ptr.add(base + SLOT_HEADER), w * h * 4)
        };
        for (y, dst) in pixels.chunks_exact_mut(w * 4).enumerate() {
            convert_row(&data[y * pitch..y * pitch + w * bpp], dst, format);
        }
        self.shm.set_u32(base + 8, w as u32);
        self.shm.set_u32(base + 12, h as u32);
        self.shm.set_u32(base + 16, aspect.to_bits());
        generation.store(g + 2, Ordering::Release);
        self.shm.seq().store(next, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn unique_name() -> String {
        static N: AtomicU32 = AtomicU32::new(0);
        format!(
            "/cart-t{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn xrgb8888_becomes_rgba() {
        let src = [0x30, 0x20, 0x10, 0x00];
        let mut dst = [0u8; 4];
        convert_row(&src, &mut dst, SrcFormat::Xrgb8888);
        assert_eq!(dst, [0x10, 0x20, 0x30, 0xff]);
    }

    #[test]
    fn rgb565_primaries() {
        let src = [0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00];
        let mut dst = [0u8; 12];
        convert_row(&src, &mut dst, SrcFormat::Rgb565);
        assert_eq!(dst, [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn rgb1555_red() {
        let src = 0x7c00u16.to_le_bytes();
        let mut dst = [0u8; 4];
        convert_row(&src, &mut dst, SrcFormat::Rgb1555);
        assert_eq!(dst, [255, 0, 0, 255]);
    }

    #[test]
    fn writer_frame_is_read_back_once() {
        let name = unique_name();
        let reader = FrameReader::create(&name).unwrap();
        let mut writer = FrameWriter::open(&name).unwrap();
        let mut out = Vec::new();
        assert!(reader.read_into(0, &mut out).is_none());

        let src = [
            1, 2, 3, 0, 4, 5, 6, 0, 0xee, 7, 8, 9, 0, 10, 11, 12, 0, 0xee,
        ];
        writer.write(&src, 2, 2, 9, SrcFormat::Xrgb8888, 1.5);
        let info = reader.read_into(0, &mut out).unwrap();
        assert_eq!(
            (info.width, info.height, info.aspect, info.seq),
            (2, 2, 1.5, 1)
        );
        assert_eq!(
            out,
            [3, 2, 1, 255, 6, 5, 4, 255, 9, 8, 7, 255, 12, 11, 10, 255]
        );
        assert!(reader.read_into(info.seq, &mut out).is_none());

        writer.write(&[0; 4], 1, 1, 4, SrcFormat::Xrgb8888, 1.0);
        assert_eq!(reader.read_into(info.seq, &mut out).unwrap().seq, 2);
    }

    #[test]
    fn oversized_frame_is_clamped() {
        let name = unique_name();
        let reader = FrameReader::create(&name).unwrap();
        let mut writer = FrameWriter::open(&name).unwrap();
        let (w, h) = (3000u32, 3u32);
        let src = vec![0x7fu8; (w * h * 4) as usize];
        writer.write(&src, w, h, (w * 4) as usize, SrcFormat::Xrgb8888, 4.0 / 3.0);
        let mut out = Vec::new();
        let info = reader.read_into(0, &mut out).unwrap();
        assert_eq!((info.width, info.height), (MAX_W, 3));
        assert_eq!(out.len(), (MAX_W * 3 * 4) as usize);
    }

    #[test]
    fn short_or_empty_input_is_ignored() {
        let name = unique_name();
        let reader = FrameReader::create(&name).unwrap();
        let mut writer = FrameWriter::open(&name).unwrap();
        writer.write(&[0; 4], 2, 2, 8, SrcFormat::Xrgb8888, 1.0);
        writer.write(&[], 0, 0, 0, SrcFormat::Xrgb8888, 1.0);
        assert!(reader.read_into(0, &mut Vec::new()).is_none());
    }

    #[test]
    fn create_is_exclusive_and_drop_unlinks() {
        let name = unique_name();
        let first = FrameReader::create(&name).unwrap();
        let err = FrameReader::create(&name).err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        drop(first);
        assert_eq!(
            FrameWriter::open(&name).err().unwrap().kind(),
            io::ErrorKind::NotFound
        );
        FrameReader::create(&name).unwrap();
    }
}
