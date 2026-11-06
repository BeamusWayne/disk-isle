use std::{
    ffi::{OsStr, OsString},
    io, mem,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_NO_MORE_FILES, FILETIME, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::*,
    System::{ProcessStatus::*, Threading::*},
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn encode(value: &OsStr) -> Vec<u8> {
    value.encode_wide().flat_map(u16::to_le_bytes).collect()
}
pub fn decode(value: &[u8]) -> OsString {
    OsString::from_wide(
        &value
            .chunks_exact(2)
            .map(|x| u16::from_le_bytes([x[0], x[1]]))
            .collect::<Vec<_>>(),
    )
}
fn wide(path: &Path) -> Vec<u16> {
    let raw = path
        .as_os_str()
        .encode_wide()
        .map(|c| if c == 47 { 92 } else { c })
        .collect::<Vec<_>>();
    let prefix = [92, 92, 63, 92];
    let mut value = if raw.starts_with(&prefix) {
        raw
    } else if raw.starts_with(&[92, 92]) {
        let mut out: Vec<u16> = "\\\\?\\UNC\\".encode_utf16().collect();
        out.extend_from_slice(&raw[2..]);
        out
    } else {
        let mut out = prefix.to_vec();
        out.extend(raw);
        out
    };
    value.push(0);
    value
}
fn open(path: &Path, access: u32) -> io::Result<Handle> {
    let name = wide(path);
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(Handle(handle))
    }
}

pub struct Entry {
    pub name: OsString,
    pub directory: bool,
    pub link: bool,
    pub logical: u64,
    pub allocated: u64,
    pub identity: Option<[u8; 16]>,
    pub estimated: bool,
}
enum Reader {
    Native {
        handle: Handle,
        buffer: Vec<u64>,
        offset: Option<usize>,
        first: bool,
    },
    Fallback(std::fs::ReadDir),
}
pub struct Directory {
    path: PathBuf,
    reader: Reader,
}
impl Directory {
    pub fn open(path: &Path) -> io::Result<Self> {
        let handle = open(path, FILE_LIST_DIRECTORY)?;
        Ok(Self {
            path: path.to_owned(),
            reader: Reader::Native {
                handle,
                buffer: vec![0; 8192],
                offset: None,
                first: true,
            },
        })
    }
    pub fn next(&mut self) -> io::Result<Option<Entry>> {
        loop {
            match &mut self.reader {
                Reader::Native {
                    handle,
                    buffer,
                    offset,
                    first,
                } => {
                    if offset.is_none() {
                        let class = if *first {
                            FileIdExtdDirectoryRestartInfo
                        } else {
                            FileIdExtdDirectoryInfo
                        };
                        let ok = unsafe {
                            GetFileInformationByHandleEx(
                                handle.0,
                                class,
                                buffer.as_mut_ptr().cast(),
                                (buffer.len() * 8) as u32,
                            )
                        };
                        if ok == 0 {
                            let error = io::Error::last_os_error();
                            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                                return Ok(None);
                            }
                            if *first && matches!(error.raw_os_error(), Some(1 | 50 | 87 | 124)) {
                                self.reader = Reader::Fallback(std::fs::read_dir(&self.path)?);
                                continue;
                            }
                            return Err(error);
                        }
                        *first = false;
                        *offset = Some(0);
                    }
                    let pos = offset.unwrap();
                    let capacity = buffer.len() * 8;
                    let name_offset = mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileName);
                    if pos + mem::size_of::<FILE_ID_EXTD_DIR_INFO>() > capacity {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "目录记录越界"));
                    }
                    let base = unsafe { buffer.as_ptr().cast::<u8>().add(pos) };
                    let info = unsafe { ptr::read_unaligned(base.cast::<FILE_ID_EXTD_DIR_INFO>()) };
                    let length = info.FileNameLength as usize;
                    if length % 2 != 0 || pos + name_offset + length > capacity {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "目录名称记录损坏",
                        ));
                    }
                    let name = unsafe {
                        std::slice::from_raw_parts(base.add(name_offset).cast::<u16>(), length / 2)
                    };
                    let name = OsString::from_wide(name);
                    if info.NextEntryOffset == 0 {
                        *offset = None;
                    } else if info.NextEntryOffset as usize >= name_offset + length
                        && pos + (info.NextEntryOffset as usize) < capacity
                    {
                        *offset = Some(pos + info.NextEntryOffset as usize);
                    } else {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "目录记录偏移损坏",
                        ));
                    }
                    if name == "." || name == ".." {
                        continue;
                    }
                    return Ok(Some(Entry {
                        name,
                        directory: info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
                        link: info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
                        logical: info.EndOfFile.max(0) as u64,
                        allocated: info.AllocationSize.max(0) as u64,
                        identity: (info.FileId.Identifier != [0; 16])
                            .then_some(info.FileId.Identifier),
                        estimated: false,
                    }));
                }
                Reader::Fallback(iterator) => {
                    let Some(entry) = iterator.next() else {
                        return Ok(None);
                    };
                    let entry = entry?;
                    let metadata = entry.metadata()?;
                    use std::os::windows::fs::MetadataExt;
                    let directory = metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0;
                    let link = metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
                    let mut allocated = metadata.len();
                    let mut identity = None;
                    let mut estimated = !directory;
                    if !directory && let Ok(handle) = open(&entry.path(), 0) {
                        let mut standard = FILE_STANDARD_INFO::default();
                        if unsafe {
                            GetFileInformationByHandleEx(
                                handle.0,
                                FileStandardInfo,
                                (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                                mem::size_of_val(&standard) as u32,
                            )
                        } != 0
                        {
                            allocated = standard.AllocationSize.max(0) as u64;
                            estimated = false;
                        }
                        let mut id = FILE_ID_INFO::default();
                        if unsafe {
                            GetFileInformationByHandleEx(
                                handle.0,
                                FileIdInfo,
                                (&mut id as *mut FILE_ID_INFO).cast(),
                                mem::size_of_val(&id) as u32,
                            )
                        } != 0
                        {
                            identity = Some(id.FileId.Identifier);
                        }
                    }
                    return Ok(Some(Entry {
                        name: entry.file_name(),
                        directory,
                        link,
                        logical: metadata.len(),
                        allocated,
                        identity,
                        estimated,
                    }));
                }
            }
        }
    }
}

pub fn cpu_seconds() -> f64 {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        );
    }
    let ticks = |v: FILETIME| ((v.dwHighDateTime as u64) << 32) | v.dwLowDateTime as u64;
    (ticks(kernel) + ticks(user)) as f64 / 10_000_000.0
}
pub fn lower_priority() {
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}
pub fn memory_peak() -> u64 {
    let mut value = PROCESS_MEMORY_COUNTERS::default();
    value.cb = mem::size_of_val(&value) as u32;
    unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut value,
            mem::size_of_val(&value) as u32,
        );
    }
    value.PeakWorkingSetSize as u64
}
pub fn space(path: &Path) -> Option<(u64, u64)> {
    let mut available = 0;
    let mut total = 0;
    let mut free = 0;
    if unsafe { GetDiskFreeSpaceExW(wide(path).as_ptr(), &mut available, &mut total, &mut free) }
        != 0
    {
        Some((total, total.saturating_sub(free)))
    } else {
        None
    }
}
pub fn drives() -> Vec<PathBuf> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| PathBuf::from(format!("{}:\\", (b'A' + i) as char)))
        .collect()
}
