use core::future::Future;
use rustix::io_uring::{io_uring_params, io_uring_setup};

struct Event;
struct LogicalClock;
struct EventBatch;

struct NodeID;
struct TopicID;

enum Foo {
	Bar,
	Baz,
}

trait ReadRes {}

trait Node {
	fn topic_create(name: &str) -> impl Future<Output = TopicID>;
	fn topic_append(topic_id: &TopicID) -> impl Future<Output = usize>;
	fn topic_read(
		topic_id: &TopicID,
		lc: &LogicalClock,
		read_res: &mut impl ReadRes,
	);
}

trait Log {
	fn enqueue(&mut self, data: &Event) -> usize; // n bytes enqueued
	fn commit(&mut self) -> impl Future<Output = usize>; // n events committed
	fn read(&self, from: usize) -> impl Iterator<Item = Event>;
}
