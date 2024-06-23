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
    const Max = struct {
        n: u64,
        fn gen(self: @This(), comptime R: anytype, rng: *R) u64 {
            return rng.random().uintLessThan(u64, self.n);
        }
    };
    const config = .{
        .total_actors = Max{ .n = 256 },
    };
};

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    const RNG = std.Random.Xoshiro256;

    const allocator = arena.allocator();
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.rand.DefaultPrng.init(seed);

    const n_actors = sim.config.total_actors.gen(RNG, &rng);

    var actors = std.AutoHashMap(Addr, Actor).init(allocator);

    for (0..n_actors) |_| {
        const addr = Addr.init(std.Random.Xoshiro256, &rng);
        try actors.putNoClobber(addr, Actor.init(addr));
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
