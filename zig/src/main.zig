const std = @import("std");
const testing = std.testing;
const Allocator = std.mem.Allocator;

const util = @import("util.zig");

const ReplicaID = enum(u128) {
    _,

    fn init(rng: anytype) @This() {
        return @enumFromInt(rng.random().int(u128));
    }
};

test "create replica ID" {
    const seed: u128 = @bitCast(std.time.nanoTimestamp());
    var rng = std.rand.DefaultPrng.init(@truncate(seed));
    _ = ReplicaID.init(&rng);
}

const event = struct {
    pub const ID = struct { origin: ReplicaID, pos: usize };
    pub const Header = struct { byte_len: usize, id: ID };

    pub const Event = struct { id: ID, payload: []const u8 };
};

const ReadCache = struct {
    buf: []u8,
    a: util.Region(u8),
    b: util.Region(u8),

    pub fn init(allocator: Allocator, capacity: usize) !@This() {
        return ReadCache{
            .buf = try allocator.alloc(u8, capacity),
            .a = util.Region(u8).zero(),
            .b = util.Region(u8).zero(),
        };
    }

    pub fn deinit(self: *@This(), allocator: Allocator) void {
        allocator.free(self.buf);
        self.* = undefined;
    }

    // TODO: delete below, just inline
    pub fn read_a(self: @This()) []const u8 {
        return self.a.read(self.buf);
    }

    pub fn read_b(self: @This()) []const u8 {
        return self.b.read(self.buf);
    }
};

test "alloc and free buf" {
    var rc = try ReadCache.init(testing.allocator, 8);
    defer rc.deinit(testing.allocator);
    try testing.expectEqualSlices(u8, rc.read_a(), &[_]u8{});
}
