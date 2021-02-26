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
