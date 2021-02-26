#![feature(label_break_value)]

use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    fs::File,
    io::{Cursor, Read},
    sync::{atomic::AtomicUsize, Arc},
    time::SystemTime,
};
use std::{
    pin::Pin,
    sync::{atomic::Ordering, Mutex},
};

pub use lazy_static::lazy_static;
use libloading::{Library, Symbol};
pub use reloady_impl::{hot_reload, init};
use symbolic::debuginfo::Object;
// use symbolic::debuginfo::Symbol;

lazy_static! {
    static ref __CRATE_NAME: Mutex<Option<&'static str>> = Mutex::new(None);
    static ref __MANIFEST_DIR: Mutex<Option<&'static str>> = Mutex::new(None);
    static ref __MOST_RECENT_VERSION: Mutex<usize> = Mutex::new(0);
    static ref __LIB_LAST_MODIFY_TIME: Mutex<SystemTime> = Mutex::new(SystemTime::UNIX_EPOCH);
    // // (*mut u8, usize) -> (usize, usize)
    // static ref __LIB_REFS: Mutex<HashMap<&'static str, (usize, usize)>> =
    //     Mutex::new(HashMap::new());
    static ref __CURRENT_LIB_REF: Mutex<Option<Library>> = Mutex::new(None);
    static ref __CURRENT_DEBUGINFO: Mutex<Option<Debuginfo>> = Mutex::new(None);
    static ref __LIB_VERSIONS: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
}

struct Debuginfo {
    symbols: Vec<DemangledSymbol>,
}

struct DemangledSymbol {
    mangled: String,
    demangled: String,
}

impl Debuginfo {
    pub fn new(obj: &Object) -> Self {
        Self {
            symbols: obj
                .symbols()
                .filter_map(|sym| sym.name().map(|x| x.to_string()))
                .map(|n| DemangledSymbol {
                    demangled: rustc_demangle::demangle(&n).to_string(),
                    mangled: n,
                })
                .collect(),
        }
    }
}

// set crate name for use later
pub fn init2(crate_name: &'static str, manifest_dir: &'static str) {
    let mut crate_name_guard = __CRATE_NAME.lock().unwrap();
    *crate_name_guard = Some(crate_name);
    let mut manifest_dir_guard = __MANIFEST_DIR.lock().unwrap();
    *manifest_dir_guard = Some(manifest_dir);
}

fn crate_name() -> &'static str {
    let crate_name = __CRATE_NAME.lock().unwrap();
    crate_name.unwrap()
}

fn manifest_dir() -> &'static str {
    let manifest_dir = __MANIFEST_DIR.lock().unwrap();
    manifest_dir.unwrap()
}

fn get_app_path() -> String {
    format!("{}/target/debug/{}", manifest_dir(), crate_name())
}

fn get_dbg_path() -> String {
    format!("{}/target/debug/{}", manifest_dir(), crate_name())
}

// #[cfg(target_os = "linux")]
// #[no_mangle]
// pub unsafe extern "C" fn __cxa_thread_atexit_impl(
//     _: *mut std::ffi::c_void,
//     _: *mut std::ffi::c_void,
//     _: *mut std::ffi::c_void,
// ) {
//     // compromise::linux::thread_atexit(func, obj, dso_symbol);
// }

// possibly update the given fn ptr
#[cfg(not(feature = "stub"))]
pub fn __update_fn<F: Copy>(
    fn_name: &'static str,
    _module_path: &'static str,
    sighash: u64,
    ptr: &Mutex<F>,
) {
    // get last updated time, and check
    let app_path = get_app_path();
    // println!("app path = {}", app_path);
    // let sym_name = format!("{}", fn_name);
    // let sym_name = if fn_name.starts_with('_') {
    //     &fn_name[1..]
    // } else {
    //     &fn_name
    // };
    // let sym_name = fn_name.to_string();
    let sym_name = format!("{}::{}", _module_path, fn_name);
    // if necessary, update this fn to latest version
    {
        let most_recent_version = {
            let mut mrv = __MOST_RECENT_VERSION.lock().unwrap();
            *mrv
        };
        let mut lib_versions = __LIB_VERSIONS.lock().unwrap();
        //
        if !lib_versions.contains_key(&sym_name) || lib_versions[&sym_name] < most_recent_version {
            if let Some(ref current_lib) = *__CURRENT_LIB_REF.lock().unwrap() {
                println!("input sighash = {}", sighash);
                if !valid_symbol(current_lib, &sym_name, sighash) {
                    panic!("ERR-PANIC: new lib's signature for {} does not match current signature, please restart!", sym_name);
                } else {
                    // update ptr
                    *ptr.lock().unwrap() = *load_function::<F>(current_lib, &sym_name);
                    // set version
                    let old_version = lib_versions.insert(sym_name.clone(), most_recent_version);
                    println!(
                        "migrated fn {} from version {} -> {}",
                        sym_name,
                        old_version.unwrap_or(0),
                        most_recent_version
                    );
                }
            }
        }
    }
    // println!("sym_name = {}", sym_name);
    let metadata = 'l: {
        loop {
            let res = std::fs::metadata(&app_path);
            match res {
                Ok(m) => break 'l m,
                _ => continue,
            }
        }
    };
    {
        let mut last_modify_time = __LIB_LAST_MODIFY_TIME.lock().unwrap();
        let modified = metadata.modified().unwrap();
        if modified > *last_modify_time {
            // update
            *last_modify_time = modified;
        } else {
            return;
        }
    }

    // 1. reload lib, while holding lib ref!!
    {
        // update version to next
        let mut new_version = __MOST_RECENT_VERSION.lock().unwrap();
        *new_version += 1;
        println!("new version = {}", new_version);
        let mut lib_ref = __CURRENT_LIB_REF.lock().unwrap();

        if let Some(old_lib) = lib_ref.take() {
            println!("dropped old lib");
            old_lib.close().unwrap();
        }

        eprintln!("info: loading new lib for function {}", sym_name);

        // SAFETY: app_path always resolves to a valid exe
        let new_lib = 'l: {
            loop {
                let res = unsafe { Library::new(get_app_path()) };
                match res {
                    Ok(l) => break 'l l,
                    Err(_) => continue,
                }
            }
        };
        *lib_ref = Some(new_lib);

        // 0. update debuginfo
        update_debuginfo();

        let lr = lib_ref.as_ref().unwrap();

        // 1.5. check symbol validity
        if !valid_symbol(lr, &sym_name, sighash) {
            panic!(
                "ERR-PANIC: new lib's signature for {} does not match current signature, please restart!",
                sym_name
            );
        }

        // 2. load new symbol
        let f = load_function::<F>(lr, &sym_name);

        // 3. swap ptr
        *ptr.lock().unwrap() = *f;

        // 4. update lib version
        let mut lib_versions = __LIB_VERSIONS.lock().unwrap();
        lib_versions.insert(sym_name, *new_version);
    }

    // // update version to next..
    // let f = find_function(&sym_name);
    // let (fptr, f_code) = load_function::<F>(f);
    // {
    //     let mut lib_refs = __LIB_REFS.lock().unwrap();
    //     let old_ref = lib_refs.insert(&sym_name, f_code);
    //     if let Some((ptr, size)) = old_ref {
    //         // free
    //         // SAFETY: ptr and size are mmapped
    //         unsafe { nix::sys::mman::munmap(ptr as _, size).unwrap() };
    //     }
    // }
    // *ptr.lock().unwrap() = fptr;
}

#[cfg(feature = "stub")]
pub fn __update_fn<F: Copy>(_: &'static str, _: &'static str, _: u64, _: &Mutex<F>) {}

#[cfg(not(feature = "stub"))]
fn update_debuginfo() {
    // load debuginfo to find symbols
    let debuginfo_bytes = {
        let mut f = File::open(get_dbg_path()).unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        buf
    };
    // let pin_bytes = Pin::new(debuginfo_bytes.into_boxed_slice());
    let debuginfo = Object::parse(&debuginfo_bytes).unwrap();
    let debuginfo = Debuginfo::new(&debuginfo);
    {
        let mut debuginfo_current = __CURRENT_DEBUGINFO.lock().unwrap();
        drop(debuginfo_current.take());
        *debuginfo_current = Some(debuginfo);
    }
    // debuginfo.obj = Object::parse(&debuginfo.bytes.as_ref()).unwrap();
}

struct DebuginfoGuard<'a> {
    guard: std::sync::MutexGuard<'a, Option<Debuginfo>>,
}

impl<'a> std::ops::Deref for DebuginfoGuard<'a> {
    type Target = Debuginfo;

    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().unwrap()
    }
}

fn get_debuginfo<'a>() -> DebuginfoGuard<'a> {
    let debuginfo_current = __CURRENT_DEBUGINFO.lock().unwrap();
    DebuginfoGuard {
        guard: debuginfo_current,
    }
}

// fn load_function<F: Copy>((addr, size): (u64, u64)) -> (F, (usize, usize)) {
//     let elf_bytes = {
//         let mut f = File::open(get_app_path()).unwrap();
//         let mut v = Vec::new();
//         f.read_to_end(&mut v).unwrap();
//         v
//     };
//     let mut cur = Cursor::new(&elf_bytes);
//     let elf = ElfFile::open_stream(&mut cur).unwrap();
//     for ref ph in elf.phdrs {
//         if ph.vaddr <= addr && ph.vaddr + ph.memsz >= addr + size {
//             // read symbol
//             let addr_offset = (addr - ph.vaddr) + ph.offset;
//             println!("resolved addr = {:x}", addr_offset);
//             let code_slice = &elf_bytes[addr_offset as _..(addr_offset + size) as _];
//             assert!(code_slice.len() == size as usize);
//             println!(
//                 "Phdr va = {:x}, pa = {:x}, wanted addr = {:x}, end addr = {:x}",
//                 ph.vaddr,
//                 ph.paddr,
//                 addr,
//                 addr + size
//             );
//             println!("Allocating size = {}", size);
//             println!("Copying bytes = {:x?}", code_slice);
//             // SAFETY: anonymous mapping with most compatible parameters
//             let ptr = unsafe {
//                 nix::sys::mman::mmap(
//                     std::ptr::null_mut(),
//                     size as _,
//                     ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
//                     MapFlags::MAP_ANONYMOUS | MapFlags::MAP_PRIVATE,
//                     -1,
//                     0,
//                 )
//             }
//             .unwrap() as *mut u8;
//             // SAFETY: Both parameters point to valid memory, and the destination has enough space.
//             unsafe { std::ptr::copy_nonoverlapping(code_slice.as_ptr(), ptr, code_slice.len()) };

//             // !! perform relocations !! important
//             // for reloc in elf.

//             // madvise the ptr to be execute
//             // SAFETY: ptr is valid
//             unsafe {
//                 nix::sys::mman::mprotect(ptr as _, code_slice.len(), ProtFlags::PROT_EXEC).unwrap();
//             };
//             // cast ptr and return
//             // SAFETY: code is valid
//             let f = unsafe { *std::mem::transmute::<_, &F>(&ptr) };
//             println!(
//                 "value of f = {:x}, value of ptr = {:x}",
//                 unsafe { *std::mem::transmute::<_, &*const u8>(&f) } as usize,
//                 ptr as usize
//             );
//             return (f, (ptr as _, code_slice.len()));
//         }
//     }
//     panic!("Failed to find phdr with code");
// }

// fn find_function(name: &str) -> (u64, u64) {
//     // load debuginfo to find symbols
//     let debuginfo_bytes = {
//         let mut f = File::open(get_dbg_path()).unwrap();
//         let mut buf = Vec::new();
//         f.read_to_end(&mut buf).unwrap();
//         buf
//     };
//     let debuginfo = Object::parse(&debuginfo_bytes).unwrap();
//     // let check_name = if name.starts_with('_') {
//     //     &name[1..]
//     // } else {
//     //     name
//     // };
//     let check_name = name;
//     for sym in debuginfo.symbol_map() {
//         if let Some(sym_name) = sym.name() {
//             // println!("trying name = {}", sym_name);
//             if sym_name == check_name {
//                 // if let Ok(demangled) = rustc_demangle::try_demangle(sym_name) {
//                 // println!("trying demangled = {}", demangled);
//                 // if demangled.to_string().starts_with(name) {
//                 // this is it!
//                 // SAFETY: validated the lib contains the given symbol
//                 // return unsafe { lib.get(name.as_bytes()).unwrap() };
//                 // }
//                 // }
//                 return (sym.address, sym.size);
//             }
//         }
//     }
//     panic!("Couldn't find symbol");
// }

#[cfg(not(feature = "stub"))]
fn load_function<'a, F>(lib: &'a Library, name: &str) -> Symbol<'a, F> {
    // load debuginfo to find symbols
    // let debuginfo_bytes = {
    //     let mut f = File::open(get_dbg_path()).unwrap();
    //     let mut buf = Vec::new();
    //     f.read_to_end(&mut buf).unwrap();
    //     buf
    // };
    // let debuginfo = Object::parse(&debuginfo_bytes).unwrap();
    // let check_name = if name.starts_with('_') {
    //     &name[1..]
    // } else {
    //     name
    // };
    // let check_name = name;
    let debuginfo = get_debuginfo();
    for sym in &debuginfo.symbols {
        // if let Some(sym_name) = sym.name() {
        // println!("trying name = {}", sym_name);
        // if sym_name == check_name {
        // if let Ok(demangled) = rustc_demangle::try_demangle(sym_name) {
        // println!("trying demangled = {}", demangled);
        if sym.demangled.starts_with(name)
            && sym
                .demangled
                .split("::")
                .all(|x| !x.ends_with("__reloady_sighash"))
        {
            // println!("using demangled = {}", sym.demangled);
            // this is it!
            // SAFETY: validated the lib contains the given symbol
            return unsafe { lib.get(sym.mangled.as_bytes()).unwrap() };
        }
        // }
        // return (sym.address, sym.size);
        // SAFETY: validated the lib contains the given symbol
        // return unsafe { lib.get(name.as_bytes()) }.unwrap();
        // }
        // }
    }
    panic!("Couldn't find symbol");
}

#[cfg(not(feature = "stub"))]
fn valid_symbol<'a>(lib: &'a Library, name: &str, sighash_val: u64) -> bool {
    // load debuginfo to find symbols
    // let debuginfo_bytes = {
    //     let mut f = File::open(get_dbg_path()).unwrap();
    //     let mut buf = Vec::new();
    //     f.read_to_end(&mut buf).unwrap();
    //     buf
    // };
    // let debuginfo = Object::parse(&debuginfo_bytes).unwrap();
    // let check_name = if name.starts_with('_') {
    //     &name[1..]
    // } else {
    //     name
    // };
    let debuginfo = get_debuginfo();
    let check_name = format!("{}__reloady_sighash", name);
    for sym in &debuginfo.symbols {
        // if let Some(sym_name) = sym.name() {
        // println!("trying name = {}", sym_name);
        // if sym_name == check_name {
        // if let Ok(demangled) = rustc_demangle::try_demangle(sym_name) {
        // println!("trying demangled = {}", demangled);
        if sym.demangled.starts_with(&check_name) {
            // this is it!
            // SAFETY: validated the lib contains the given symbol
            let sym_value: Symbol<fn() -> u64> =
                unsafe { lib.get(sym.mangled.as_bytes()).unwrap() };
            let sym_value = (*sym_value)();
            println!("found sym value = {}, input = {}", sym_value, sighash_val);
            return sym_value == sighash_val;
        }
        // }
        // return (sym.address, sym.size);
        // SAFETY: validated the lib contains the given symbol
        // return unsafe { lib.get(name.as_bytes()) }.unwrap();
        // }
        // }
    }
    false
    // panic!("Couldn't find symbol");
}
