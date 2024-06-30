const std = @import("std");

const mem = struct {
    pub const Word = u64;
};

const Addr = struct {
    words: [2]u64,
    fn init(comptime R: type, rng: *R) @This() {
        return .{
            .words = .{ rng.random().int(u64), rng.random().int(u64) },
        };
    }
};

comptime {
    const alignment = @alignOf(Addr);
    std.debug.assert(alignment == 8);
}

const Actor = struct {
    addr: Addr,

    fn init(addr: Addr) @This() {
        return .{ .addr = addr };
    }
};

const Event = struct {
    const ID = struct { origin: Addr, logical_pos: usize };

    id: ID,
    payload: []mem.Word,
};

const Msg = struct {
    const Inner = union(enum) { sync_res: []Event };
};

const sim = struct {
    const Range = struct {
        at_least: u64,
        at_most: u64,
        fn gen(self: @This(), comptime R: anytype, rng: *R) u64 {
            return rng.random().intRangeAtMost(u64, self.at_least, self.at_most);
        }
    };

    fn range(at_least: u64, at_most: u64) Range {
        return .{ .at_least = at_least, .at_most = at_most };
    }

    fn max(n: u64) Range {
        return .{ .at_least = 0, .at_most = n };
    }

    const config = .{
        .n_actors = max(256),
        .payload_size = range(0, 4096),
        .msg_len = range(0, 64),
        .source_msgs_per_actor = range(0, 10_000),
    };

    // An environment, representing some source of messages, and an actor
    const Env = struct {
        actor: Actor,
        msg_lens: std.ArrayList(usize),
        payload_sizes: std.ArrayList(usize),

        fn init(
            comptime R: anytype,
            rng: *R,
            allocator: std.mem.Allocator,
        ) @This() {
            const msg_lens = allocator.alloc(
                u64,
                config.source_msgs_per_actor.gen(R, &rng),
            );
            for (msg_lens) |*msg_len| {
                msg_len.* = config.msg_len.gen(R, &rng);
            }

            const payload_sizes = allocator.alloc(
                u64,
                config.payload_size.gen(R, &rng),
            );
            for (payload_sizes) |*payload_size| {
                payload_size.* = config.payload_size.gen(R, &rng);
            }

            const result = .{
                .actor = Actor.init(Addr.init(rng)),
                .msg_lens = std.ArrayList.fromOwnedSlice(msg_lens),
                .payload_sizes = std.ArrayList.fromOwnedSlice(payload_sizes),
            };

            return result;
        }

        fn deinit(self: *@This(), allocator: std.mem.Allocator) void {
            self.msg_lens.deinit(allocator);
            self.payload_sizes.deinit(allocator);
        }
    };
};

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    const RNG = std.Random.Xoshiro256;

    const allocator = arena.allocator();
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.rand.DefaultPrng.init(seed);

    const n_actors = sim.config.n_actors.gen(RNG, &rng);

    var actors = std.AutoHashMap(Addr, Actor).init(allocator);

    for (0..n_actors) |_| {
        const env = sim.Env.init(
            std.Random.Xoshiro256,
            &rng,
            std.testing.allocator,
        );
        try actors.putNoClobber(env.addr, env);
    }

    std.debug.print("seed = {d}\n", .{seed});
    std.debug.print("Running simulation with:\n", .{});
    std.debug.print("- {d} actors\n", .{actors.count()});
}

test "simple test" {
    var list = std.ArrayList(i32).init(std.testing.allocator);
    defer list.deinit();
    try list.append(42);
    try std.testing.expectEqual(@as(i32, 42), list.pop());
}
