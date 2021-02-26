#![feature(link_args)]
#![feature(linkage)]

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

fn main() {
    reloady::init!();
    loop {
        println!("result of test = {}, test2 =", test(&NUMBER),);
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

static NUMBER: AtomicUsize = AtomicUsize::new(0);

#[reloady::hot_reload]
fn test(au: &AtomicUsize) -> usize {
    let res = au.load(Ordering::SeqCst);
    let res = res + 3;
    au.store(res, Ordering::SeqCst);
    res
}
