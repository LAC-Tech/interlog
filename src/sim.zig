const std = @import("std");
const lib = @import("lib.zig");
const assert = std.debug.assert;
const core = lib.core;

const ArrayListUnmanaged = std.ArrayListUnmanaged;

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
    // Currently just something "big enough", later handle disk overflow
    .storage_capacity = 10_000_000,
};

// Represents a local source of data for an actor
const PayloadSrc = struct {
    msg_lens: ArrayListUnmanaged(usize),
    payload_sizes: ArrayListUnmanaged(usize),

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
            .msg_lens = ArrayListUnmanaged(usize).fromOwnedSlice(msg_lens),
            .payload_sizes = ArrayListUnmanaged(usize).fromOwnedSlice(
                payload_sizes,
            ),
        };
    }

    fn deinit(self: *@This(), allocator: std.mem.Allocator) void {
        self.msg_lens.deinit(allocator);
        self.payload_sizes.deinit(allocator);
    }

    // TODO: Useless name. who cares if it 'pops'? the point is this generates
    // payload lens
    // TODO: just have it generate all the data?
    pub fn popPayloadLens(
        self: *@This(),
        buf: *ArrayListUnmanaged(usize),
    ) void {
        if (self.msg_lens.popOrNull()) |msg_len| {
            for (msg_len) |_| {
                const payload_size = self.payload_sizes.pop();
                buf.appendAssumeCapacity(payload_size);
            }
        }
    }
};

const Storage = struct {
    bytes: ArrayListUnmanaged(u8),
    fn init(buf: []u8) @This() {
        return .{ .bytes = ArrayListUnmanaged(u8).fromOwnedSlice(buf) };
    }

    pub fn append(self: *@This(), bytes: []const u8) void {
        self.bytes.appendSliceAssumeCapacity(bytes);
    }
};

// An environment, representing some source of messages, and a log
// TODO: this object is stupid. its only purpose is to group
pub const Env = struct {
    log: core.Log(Storage),
    payload_src: PayloadSrc,

    pub fn init(
        comptime R: anytype,
        rng: *R,
        allocator: std.mem.Allocator,
    ) !@This() {
        const storage = Storage.init(
            try allocator.alloc(u8, config.storage_capacity),
        );

        const addr = core.Addr.init(R, rng);

        const heap_mem = .{
            .enqd_offsets = try allocator.alloc(
                core.StorageOffset,
                config.msg_len.at_most,
            ),
            .enqd_events = try allocator.alloc(
                u8,
                config.msg_len.at_most * config.payload_size.at_most,
            ),
            .cmtd_offsets = try allocator.alloc(core.StorageOffset, 1_000_000),
            .cmtd_acqs = try allocator.create([std.math.maxInt(u16)]core.Addr),
        };
        return .{
            .log = try core.Log(Storage).init(addr, storage, heap_mem),
            .payload_src = try PayloadSrc.init(R, rng, allocator),
        };
    }

    pub fn deinit(self: *@This(), allocator: std.mem.Allocator) void {
        self.payload_src.deinit(allocator);
    }
};

pub const Stats = struct {
    total_cmtd_events: usize,
    total_commits: usize,

    pub fn init() @This() {
        return .{ .total_cmtd_events = 0, .total_commits = 0 };
    }

    pub fn update(self: *@This(), n_events_cmtd: usize) void {
        // Recall: zig does runtime checks for overflow by default
        self.total_cmtd_events += n_events_cmtd;
        self.total_commits += 1;
    }
};

test "set up and tear down simulated env" {
    const seed: u64 = std.crypto.random.int(u64);
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    var rng = std.Random.Pcg.init(seed);
    const allocator = arena.allocator();
    _ = try Env.init(std.Random.Pcg, &rng, allocator);
}
