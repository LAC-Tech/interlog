const std = @import("std");
const lib = @import("lib.zig");
const assert = std.debug.assert;
const Log = lib.Log;
const Addr = lib.Addr;
const FixVec = lib.FixVec;
const StorageOffset = lib.StorageOffset;

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
    .n_actors = max(8),
    .payload_size = range(0, 4096),
    .msg_len = range(0, 16),
    .source_msgs_per_actor = range(0, 100),
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

        var sum_lens: u64 = 0;
        for (msg_lens) |len| sum_lens += len;

        const payload_sizes = try allocator.alloc(u64, sum_lens);
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

const Storage = struct {
    pub fn init() @This() {
        return .{};
    }
};

// An environment, representing some source of messages, and a log
pub const Env = struct {
    log: Log(Storage),
    payload_src: PayloadSrc,

    pub fn init(
        comptime R: anytype,
        rng: *R,
        allocator: std.mem.Allocator,
    ) !@This() {
        const storage = Storage.init();
        const buffers = .{
            .enqueued = .{
                .offsets = try allocator.alloc(
                    StorageOffset,
                    config.msg_len.at_most,
                ),
                .events = try allocator.alloc(
                    u8,
                    config.msg_len.at_most * config.payload_size.at_most,
                ),
            },
            .committed = .{
                .offsets = try allocator.alloc(StorageOffset, 1_000_000),
            },
        };
        return .{
            .log = Log(Storage).init(
                Addr.init(R, rng),
                storage,
                buffers,
            ),
            .payload_src = try PayloadSrc.init(R, rng, allocator),
        };
    }

    pub fn popPayloadLens(
        self: *@This(),
        buf: *std.ArrayListUnmanaged(usize),
    ) void {
        if (self.payload_src.msg_lens.popOrNull()) |msg_len| {
            for (msg_len) |_| {
                //if (self.payload_src.payload_sizes.popNull()) |payload_size| {
                //    buf.appendAssumeCapacity(payload_size);
                //}
                const payload_size = self.payload_src.payload_sizes.pop();
                buf.appendAssumeCapacity(payload_size);
            }
        }
    }

    pub fn deinit(self: *@This(), allocator: std.mem.Allocator) void {
        self.payload_src.deinit(allocator);
    }
};
