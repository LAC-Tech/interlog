const std = @import("std");
const builtin = @import("builtin");
const testing = std.testing;
const Allocator = std.mem.Allocator;

const util = @import("util.zig");

comptime {
    if (builtin.target.cpu.arch.endian() != .Little) {
        @compileError("Big Endian is not supported");
    }

    if (builtin.target.ptrBitWidth() != 64) {
        @compileError("This has only been tested on 64 bit machines");
    }
}

const ReplicaID = enum(u128) {
    _,

    fn init(rng: anytype) @This() {
        return @enumFromInt(rng.random().int(u128));
    }
};

fn rng_seed() u64 {
    const seed: u128 = @bitCast(std.time.nanoTimestamp());
    return @truncate(seed);
}

test "create replica ID" {
    var rng = std.rand.DefaultPrng.init(rng_seed());
    _ = ReplicaID.init(&rng);
}

const event = struct {
    pub const ID = struct { origin: ReplicaID, pos: usize };
    pub const Header = struct { byte_len: usize, id: ID };

    comptime {
        if (@sizeOf(Header) != 24) {
            @compileError("event header sized has changed");
        }
    }

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
    //const Letter = enum(u8) {
    //    a,
    //    b,
    //    c,
    //    d,
    //    e,
    //    f,
    //    h,
    //    i,
    //    k,
    //    l,
    //    n,
    //    o,
    //    p,
    //    s,
    //    t,
    //    u,
    //    w,
    //    y,
    //};

    //var rng = std.rand.DefaultPrng.init(rng_seed());
    //const replica_id = ReplicaID.init(&rng);
    //_ = replica_id;

    //var ja: util.JaggedArray(Letter) = util.JaggedArray(Letter).init(testing.allocator);

    //try ja.append(&[_]Letter{ .w, .h, .o });

    var rc = try ReadCache.init(testing.allocator, 8);
    defer rc.deinit(testing.allocator);
    try testing.expectEqualSlices(u8, rc.read_a(), &[_]u8{});
}
