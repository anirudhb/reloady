/*
 * reloady - Simple, performant hot-reloading for Rust.
 * Copyright (C) 2021 the reloady authors
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */
#![feature(label_break_value)]

use std::{collections::HashMap, fs::File, io::Read, sync::Mutex, time::SystemTime};

#[cfg(feature = "unstub")]
pub use lazy_static::lazy_static;
#[cfg(feature = "unstub")]
use libloading::{Library, Symbol};
#[cfg(feature = "unstub")]
use symbolic::debuginfo::Object;

pub use reloady_impl::{hot_reload, init};

#[cfg(feature = "unstub")]
lazy_static! {
    static ref __CRATE_NAME: Mutex<Option<&'static str>> = Mutex::new(None);
    static ref __MANIFEST_DIR: Mutex<Option<&'static str>> = Mutex::new(None);
    static ref __MOST_RECENT_VERSION: Mutex<usize> = Mutex::new(0);
    static ref __LIB_LAST_MODIFY_TIME: Mutex<SystemTime> = Mutex::new(SystemTime::UNIX_EPOCH);
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
    #[cfg(feature = "unstub")]
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
#[cfg(feature = "unstub")]
pub fn init2(crate_name: &'static str, manifest_dir: &'static str) {
    let mut crate_name_guard = __CRATE_NAME.lock().unwrap();
    *crate_name_guard = Some(crate_name);
    let mut manifest_dir_guard = __MANIFEST_DIR.lock().unwrap();
    *manifest_dir_guard = Some(manifest_dir);
}
#[cfg(not(feature = "unstub"))]
pub fn init2(_: &'static str, _: &'static str) {}

#[cfg(feature = "unstub")]
fn crate_name() -> &'static str {
    let crate_name = __CRATE_NAME.lock().unwrap();
    crate_name.unwrap()
}

#[cfg(feature = "unstub")]
fn manifest_dir() -> &'static str {
    let manifest_dir = __MANIFEST_DIR.lock().unwrap();
    manifest_dir.unwrap()
}

#[cfg(feature = "unstub")]
fn get_app_path() -> String {
    format!("{}/target/debug/{}", manifest_dir(), crate_name())
}

#[cfg(feature = "unstub")]
fn get_dbg_path() -> String {
    format!("{}/target/debug/{}", manifest_dir(), crate_name())
}

// possibly update the given fn ptr
#[cfg(feature = "unstub")]
pub fn __update_fn<F: Copy>(
    fn_name: &'static str,
    _module_path: &'static str,
    sighash: u64,
    ptr: &Mutex<F>,
) {
    // get last updated time, and check
    let app_path = get_app_path();
    let sym_name = format!("{}::{}", _module_path, fn_name);
    // if necessary, update this fn to latest version
    {
        let most_recent_version = {
            let mrv = __MOST_RECENT_VERSION.lock().unwrap();
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
}

#[cfg(not(feature = "unstub"))]
pub fn __update_fn<F: Copy>(_: &'static str, _: &'static str, _: u64, _: &Mutex<F>) {}

#[cfg(feature = "unstub")]
fn update_debuginfo() {
    // load debuginfo to find symbols
    let debuginfo_bytes = {
        let mut f = File::open(get_dbg_path()).unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        buf
    };
    let debuginfo = Object::parse(&debuginfo_bytes).unwrap();
    let debuginfo = Debuginfo::new(&debuginfo);
    {
        let mut debuginfo_current = __CURRENT_DEBUGINFO.lock().unwrap();
        drop(debuginfo_current.take());
        *debuginfo_current = Some(debuginfo);
    }
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

#[cfg(feature = "unstub")]
fn get_debuginfo<'a>() -> DebuginfoGuard<'a> {
    let debuginfo_current = __CURRENT_DEBUGINFO.lock().unwrap();
    DebuginfoGuard {
        guard: debuginfo_current,
    }
}

#[cfg(feature = "unstub")]
fn load_function<'a, F>(lib: &'a Library, name: &str) -> Symbol<'a, F> {
    let debuginfo = get_debuginfo();
    for sym in &debuginfo.symbols {
        if sym.demangled.starts_with(name)
            && sym
                .demangled
                .split("::")
                .all(|x| !x.ends_with("__reloady_sighash"))
        {
            // SAFETY: validated the lib contains the given symbol
            return unsafe { lib.get(sym.mangled.as_bytes()).unwrap() };
        }
    }
    panic!("Couldn't find symbol");
}

#[cfg(feature = "unstub")]
fn valid_symbol<'a>(lib: &'a Library, name: &str, sighash_val: u64) -> bool {
    let debuginfo = get_debuginfo();
    let check_name = format!("{}__reloady_sighash", name);
    for sym in &debuginfo.symbols {
        if sym.demangled.starts_with(&check_name) {
            // SAFETY: validated the lib contains the given symbol
            let sym_value: Symbol<fn() -> u64> =
                unsafe { lib.get(sym.mangled.as_bytes()).unwrap() };
            let sym_value = (*sym_value)();
            println!("found sym value = {}, input = {}", sym_value, sighash_val);
            return sym_value == sighash_val;
        }
    }
    false
}
