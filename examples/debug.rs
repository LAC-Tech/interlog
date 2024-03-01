extern crate interlog;

use core::ops::Deref;
use interlog::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn main() {
	let tmp_dir =
		TempDir::with_prefix("interlog-").expect("failed to open temp file");

	let mut rng = rand::thread_rng();
	let config = replica::Config {
		index_capacity: 128,
		read_cache_capacity: unit::Byte(1024),
	};

	let mut replica = replica::Local::new(tmp_dir.path(), &mut rng, config)
		.expect("failed to open file");

	let mut client =
		replica::io_bus::IOBus::new(unit::Byte(0x400), unit::Byte(0x400));

	let ess: &[&[&[u8]]] = &[&[&[]], &[]];

	for actual in ess {
		let vals = actual.iter().map(Deref::deref);
		replica
			.local_write(&mut client, vals)
			.expect("failed to write to replica");

		replica.read(&mut client, 0).expect("failed to read to file");

		let expected: Vec<_> =
			client.read_out().map(|e| e.payload.to_vec()).collect();

		dbg!(actual);
		dbg!(&expected);

		assert_eq!(&expected, actual);
	}
}
