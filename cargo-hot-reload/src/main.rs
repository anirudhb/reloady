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
use std::{
    env::current_dir,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result};
use clap::Clap;
#[macro_use]
extern crate log;
use notify::{RecursiveMode, Watcher};
use pretty_env_logger::env_logger::Env;
use serde_derive::{Deserialize, Serialize};

#[derive(Clap)]
#[clap(
    version = "0.1",
    author = "Anirudh Balaji <anirudhb@users.noreply.github.com>",
    setting = clap::AppSettings::TrailingVarArg
)]
struct Opts {
    args: Vec<OsString>,
}

#[derive(Serialize, Deserialize)]
struct CargoToml {
    package: CrateInfo,
}

#[derive(Serialize, Deserialize, Debug)]
struct CrateInfo {
    name: String,
}

fn main() -> Result<()> {
    let env = Env::default().default_filter_or("info");
    pretty_env_logger::env_logger::init_from_env(env);
    info!("initialized pretty env logger");

    let args = Opts::parse();
    let cargo_toml_dir = walk_toml_dir()?;
    let crate_info = parse_toml(cargo_toml_dir.join("Cargo.toml"))?;
    info!("Got crate info = {:?}", crate_info);

    info!("Running initial build...");
    reload(&cargo_toml_dir, &crate_info, false)?;

    let src_dir = cargo_toml_dir.join("src");
    info!("Starting watcher at path {}", src_dir.to_string_lossy());

    let (tx, rx) = std::sync::mpsc::channel();

    let mut watcher = notify::watcher(tx, Duration::from_secs(1))?;

    watcher.watch(&src_dir, RecursiveMode::Recursive)?;

    info!("Listening...");

    let mut cmd = Command::new(get_exe_name(&cargo_toml_dir, &crate_info));
    let mut handle = cmd.args(&args.args).spawn()?;

    let _th = std::thread::spawn(move || loop {
        use notify::DebouncedEvent::*;
        match rx.recv() {
            Ok(e) => match e {
                Create(..) | Write(..) | Remove(..) | Rename(..) | Rescan => {
                    info!("Change detected, reloading...");
                    match reload(&cargo_toml_dir, &crate_info, true) {
                        Ok(_) => {}
                        Err(e) => error!("{}", e),
                    }
                }
                _ => {}
            },
            Err(e) => error!("{}", e),
        }
    });

    std::process::exit(handle.wait()?.code().unwrap_or_default());
}

fn get_exe_name<P: AsRef<Path>>(toml_dir: P, info: &CrateInfo) -> PathBuf {
    toml_dir.as_ref().join("target/debug").join(&info.name)
}

fn reload<P: AsRef<Path>>(toml_dir: P, info: &CrateInfo, stub: bool) -> Result<()> {
    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd.args(&["build", "--quiet", "--features"]);
    let mut features = vec!["reloady/enabled"];
    if !stub {
        features.push("reloady/unstub");
    }
    cargo_cmd.arg(features.join(","));
    let mut cargo_inst = cargo_cmd
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;
    let cargo_status = cargo_inst.wait()?;
    let cmd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mkexeloadable");
    let exe_name = get_exe_name(toml_dir, info);
    let cmd_inst = Command::new(cmd).arg(exe_name).status()?;
    if cmd_inst.success() && cargo_status.success() {
        info!("Reload success!");
        Ok(())
    } else {
        info!("Reload failure :(");
        let mut cargo_stderr = String::new();
        cargo_inst
            .stderr
            .unwrap()
            .read_to_string(&mut cargo_stderr)?;
        info!("cargo err = {}", cargo_stderr);
        Err(anyhow::anyhow!("command failed"))
    }
}

fn walk_toml_dir() -> Result<PathBuf> {
    let mut current_dir = current_dir()?;
    info!("Walking dir {}", current_dir.to_string_lossy());
    while !current_dir.join("Cargo.toml").exists() {
        current_dir = current_dir
            .parent()
            .context("Get parent directory")?
            .to_path_buf();
        info!("Walking dir {}", current_dir.to_string_lossy());
    }
    Ok(current_dir)
}

fn parse_toml<P: AsRef<Path>>(path: P) -> Result<CrateInfo> {
    let contents = std::fs::read_to_string(path)?;
    let ctoml = toml::from_str::<CargoToml>(&contents)?;
    Ok(ctoml.package)
}
