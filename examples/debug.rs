extern crate interlog;

use core::ops::Deref;
use interlog::{ReplicaID, FixVec};

fn main() {
    let minimal_failing = vec![vec![], vec![], vec![0]];
    // Setup 
    let mut rng = rand::thread_rng();
    let replica_id = ReplicaID::new(&mut rng);
    
    let mut buf = FixVec::new(0x400);

    // Pre conditions
    assert_eq!(buf.len(), 0, "buf should start empty");
    assert!(buf.read_event(0.into()).is_none(), "should contain no event");
    
    let vals = minimal_failing.iter().map(Deref::deref);
    buf.append_events((0.into(), 0.into()), replica_id, vals)
        .expect("buf should have enough");

    // Post conditions
    let actual: Vec<_> = buf.into_iter().map(|e| e.val).collect();
    assert_eq!(&actual, &minimal_failing);
}
