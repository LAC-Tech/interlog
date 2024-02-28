extern crate interlog;

use core::ops::Deref;
use interlog::*;
use tempfile::TempDir;

fn main() {
    let es: &[&[u8]] = &[&[]];
    let tmp_dir = TempDir::with_prefix("interlog-")
        .expect("failed to open temp file");

    let mut rng = rand::thread_rng();
    let config = replica::Config {
        index_capacity: 16,
        read_cache_capacity: interlog::unit::Byte(256),
        txn_write_buf_capacity: interlog::unit::Byte(256)
    };

    let mut replica = replica::Local::new(tmp_dir.path(), &mut rng, config)
        .expect("failed to open file");

    let vals = es.iter().map(Deref::deref);
    replica.local_write(vals)
        .expect("failed to write to replica");

    let mut read_buf = utils::FixVec::new(0x400);
    replica.read(&mut read_buf, 0).expect("failed to read to file");

    let events: Vec<_> = read_buf.into_iter()
        .map(|e| e.val.to_vec())
        .collect();

    assert_eq!(events, es);
}
