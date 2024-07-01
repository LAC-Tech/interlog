const std = @import("std");
const RNG = std.Random.Xoshiro256;

const ONE_DAY_IN_MS: u64 = 1000 * 60 * 60 * 24;

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

    // Represents a local source of data for an actor
    const PayloadSrc = struct {
        msg_lens: std.ArrayListUnmanaged(usize),
        payload_sizes: std.ArrayListUnmanaged(usize),

        fn init(
            comptime R: anytype,
            rng: *R,
            allocator: std.mem.Allocator,
        ) !@This() {
            const msg_lens = try allocator.alloc(
                u64,
                config.source_msgs_per_actor.gen(R, rng),
            );
            for (msg_lens) |*msg_len| {
                msg_len.* = config.msg_len.gen(R, rng);
            }

            const payload_sizes = try allocator.alloc(
                u64,
                config.payload_size.gen(R, rng),
            );
            for (payload_sizes) |*payload_size| {
                payload_size.* = config.payload_size.gen(R, rng);
            }

            return .{
                .msg_lens = std.ArrayListUnmanaged(usize).fromOwnedSlice(
                    msg_lens,
                ),
                .payload_sizes = std.ArrayListUnmanaged(usize).fromOwnedSlice(
                    payload_sizes,
                ),
            };
        }

        fn deinit(self: *@This(), allocator: std.mem.Allocator) void {
            self.msg_lens.deinit(allocator);
            self.payload_sizes.deinit(allocator);
        }
    };

    // An environment, representing some source of messages, and an actor
    const Env = struct {
        actor: Actor,
        payload_src: PayloadSrc,

        fn init(
            comptime R: anytype,
            rng: *R,
            allocator: std.mem.Allocator,
        ) !@This() {
            return .{
                .actor = Actor.init(Addr.init(R, rng)),
                .payload_src = try PayloadSrc.init(R, rng, allocator),
            };
        }

        fn deinit(self: *@This(), allocator: std.mem.Allocator) void {
            self.payload_src.deinit(allocator);
        }
    };

    //fn RandPayloadIterator(comptime R: anytype) type {
    //    return struct {
    //        payload_buf: [config.payload_size.at_most]u8,
    //        sizes: std.ArrayListUnmanaged(usize),
    //        rng: *R,
    //        //
    //        fn init(
    //            rng: *R, allocator: std.mem.Allocator
    //        ) @This() {
    //            return .{
    //                .payload_buf = undefined,
    //                .sizes = std.ArrayListUnamanged(usize).initCapacity(
    //                    allocator,
    //                    config.msg_len.at_most,
    //                ),
    //                .rng = rng,
    //            };
    //        }

    //        fn populate(self: *@This(), msg_len, payload_sizes: *std.Array)

    //        fn next(self: *@This()) ?[]const u8 {
    //            const payload_size = self.payload_sizes().popOrNull()
    //            self.rng.random().bytes(se)
    //        }
    //    };
    //}
};

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const allocator = gpa.allocator();
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.rand.DefaultPrng.init(seed);

    const n_actors = sim.config.n_actors.gen(RNG, &rng);

    var envs = std.AutoHashMap(Addr, sim.Env).init(allocator);

    for (0..n_actors) |_| {
        const env = try sim.Env.init(RNG, &rng, allocator);
        try envs.putNoClobber(env.actor.addr, env);
    }

    //var payload_buf: [sim.config.payload_size.at_most]u8 = undefined;
    std.debug.print("seed = {d}\n", .{seed});
    std.debug.print("Running simulation with:\n", .{});
    std.debug.print("- {d} actors\n", .{envs.count()});

    var i: u64 = 0;
    while (i < ONE_DAY_IN_MS) : (i += 10) {}
}

test "set up and tear down sim env" {
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.rand.DefaultPrng.init(seed);
    var env = try sim.Env.init(std.Random.Xoshiro256, &rng, std.testing.allocator);
    defer env.deinit(std.testing.allocator);
}
