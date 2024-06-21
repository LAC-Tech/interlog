use hashbrown::HashMap;
use interlog_core::*;

fn main() {
	let xs = vec![1, 2, 3];
	dbg!(xs);
}

type Addr = u64;

struct FaultyNetwork(HashMap<Addr, Box<dyn Msg<Addr>>>);
