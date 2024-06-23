use std::collections::HashMap;
use interlog_core::*;
use rand::prelude::*;

mod config {
	struct Max(usize);

	impl Max {
		fn gen<R: rand::Rng>(&self, rng: &mut R) -> usize {
			rng.gen_range(0..self.0)
		}
	}
	pub const N_ACTORS: Max = Max(256);
}

fn main() {
	let mut rng = rand::thread_rng();
	let args: Vec<String> = std::env::args().collect();
	let seed: u64 = args.get(1)
		.map(|s| s.parse::<u64>().expect(""))
		.unwrap_or_else(|| rng.gen());

	let actors = HashMap::from([

	]);


}
