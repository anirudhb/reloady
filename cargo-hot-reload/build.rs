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
fn main() {
    println!("cargo:rerun-if-changed=mkexeloadable.c");
    let mut cmd = cc::Build::new()
        .cpp(true)
        .include(".")
        .get_compiler()
        .to_command();
    cmd.args(&["-omkexeloadable", "mkexeloadable.c"]);
    assert!(cmd.status().unwrap().success());
}
