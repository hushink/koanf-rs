use koanf_rs::memory::Memory;
use koanf_rs::prelude::*;

fn main() {
    let _ = Config::builder().watched(Memory::empty());
}
