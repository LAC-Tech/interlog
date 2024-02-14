extern crate interlog;

use core::ops::Deref;
use interlog::*;

fn main() {
    let e = [0u8; 8];

    // Setup
    let mut rng = rand::thread_rng();
    let mut buf = FixVec::new(256);
    let replica_id = ReplicaID::new(&mut rng);
    let event = Event {id: ID::new(replica_id, 0), val: &e};

    // Pre conditions
    assert_eq!(buf.len(), 0, "buf should start empty");
    assert!(buf.get(0).is_none(), "should contain no event");

    println!("\nAPPEND\n");
    // Modifying
    buf.append_event(&event).expect("buf should have enough");

    println!("\nREAD\n");
    // Post conditions
    let actual = buf.read_event(0.into())
        .expect("one event to be at 0");
    assert_eq!(actual.val, &e);
}
