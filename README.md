# reloady
Simple, performant hot-reloading for Rust.

Requires Rust nightly and only works on Linux for now.

## installing CLI
To install the CLI helper `cargo hot-reload`:
```
cargo install --path cargo-hot-reload
```

## running examples
To run an example, run `cargo hot-reload` inside its directory.

Note: initial builds may take a while to complete at first, but will be fast afterwards.

Reloady's performance goal is to hot-reload in under 2 seconds, which it currently achieves.

## usage

First, add the dependency to your `Cargo.toml` (reloady is not published on crates.io yet):
```
[dependencies]
reloady = { path = "../../reloady" }
```

Next, add the following features to the top of your lib.rs or main.rs:
```
#![feature(link_args)]
#![feature(linkage)]
```

Lastly, annotate any function you would like to hot-reload with the attribute:
```
#[reloady::hot_reload]
fn hot_reload_me() {
    /* ... */
}
```

And hot reload your code with:
```
$ cargo hot-reload
```

Note that functions are only reloaded when they are called, so reloady works best when it is annotating a function that is called in a loop.
For more information on this, see examples.

## features

* hot reloads in &lt;2s
* zero-cost when not hot-reloading (compiled out entirely!)
* (coming soon) hot reloading structs/state