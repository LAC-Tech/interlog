const std = @import("std");
const lib = @import("lib.zig");
const Actor = lib.Actor;
const Addr = lib.Addr;

const Range = struct {
    at_least: u64,
    at_most: u64,
    pub fn gen(self: @This(), comptime R: anytype, rng: *R) u64 {
        return rng.random().intRangeAtMost(u64, self.at_least, self.at_most);
    }
};

fn range(at_least: u64, at_most: u64) Range {
    return .{ .at_least = at_least, .at_most = at_most };
}

fn max(n: u64) Range {
    return .{ .at_least = 0, .at_most = n };
}

pub const config = .{
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
pub const Env = struct {
    actor: Actor,
    payload_src: PayloadSrc,

    pub fn init(
        comptime R: anytype,
        rng: *R,
        allocator: std.mem.Allocator,
    ) !@This() {
        return .{
            .actor = Actor.init(Addr.init(R, rng)),
            .payload_src = try PayloadSrc.init(R, rng, allocator),
        };
    }

    pub fn deinit(self: *@This(), allocator: std.mem.Allocator) void {
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
