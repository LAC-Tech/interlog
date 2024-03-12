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

    fn read(bytes: []const u8, offset: usize) Event {
        const header_region = util.Region.init(offset, @sizeOf(Header));
        const header_bytes: []u8 = header_region.read(bytes);
        const header: Header = @bitCast(header_bytes);

        const payload_region = header_region.right_adjacent(header.byte_len);
        const payload = payload_region.read(bytes);

        return .{ .id = header.id, .payload = payload };
    }
};

const ReadCache = struct {
    mem: []u8,
    logical_start: usize,
    a: util.Region(u8),
    b: util.Region(u8),

    pub fn init(allocator: Allocator, capacity: usize) !@This() {
        return ReadCache{
            .mem = try allocator.alloc(u8, capacity),
            .logical_start = 0,
            .a = util.Region(u8).zero(),
            .b = util.Region(u8).zero(),
        };
    }

    pub fn deinit(self: *@This(), allocator: Allocator) void {
        allocator.free(self.mem);
        self.* = undefined;
    }

    fn set_logical_start(self: *@This(), es: []const u8) void {
        self.logical_start = event.read(es, 0).id.pos;
    }

    fn new_a_byte_pos(self: @This(), es: []const u8) ?usize {
        const new_b_end = self.b.end + es.len;
        _ = new_b_end;

        var result: ?usize = null;
        _ = result;

        // return inside the loop once you find it, otherwise return null

        return null;
    }

    // TODO: delete below, just inline
    pub fn read_a(self: @This()) []const u8 {
        return self.a.read(self.mem);
    }

    pub fn read_b(self: @This()) []const u8 {
        return self.b.read(self.mem);
    }
};

test "alloc and free buf" {
    const Letter = enum(u192) {
        a,
        b,
        c,
        d,
        e,
        f,
        h,
        i,
        k,
        l,
        n,
        o,
        p,
        s,
        t,
        u,
        w,
        y,
    };

    var rng = std.rand.DefaultPrng.init(rng_seed());
    const replica_id = ReplicaID.init(&rng);
    _ = replica_id;

    var ja: util.JaggedArray(Letter) = util.JaggedArray(Letter).init(testing.allocator);

    defer ja.deinit();
    try ja.append(&[_]Letter{ .w, .h, .o });
    try ja.append(&[_]Letter{ .i, .s });
    try ja.append(&[_]Letter{ .t, .h, .i, .s });
    try ja.append(&[_]Letter{ .d, .o, .i, .n });
    try ja.append(&[_]Letter{ .t, .h, .i, .s });
    try ja.append(&[_]Letter{ .s, .y, .n, .t, .h, .e, .t, .i, .c });
    try ja.append(&[_]Letter{ .t, .y, .p, .e });
    try ja.append(&[_]Letter{ .o, .f });
    try ja.append(&[_]Letter{ .a, .l, .p, .h, .a });
    try ja.append(&[_]Letter{ .b, .e, .t, .a });
    try ja.append(&[_]Letter{ .p, .s, .y, .c, .h, .e, .d, .e, .l, .i, .c });
    try ja.append(&[_]Letter{ .f, .u, .n, .k, .i, .n });

    var rc = try ReadCache.init(testing.allocator, 8);
    defer rc.deinit(testing.allocator);
    try testing.expectEqualSlices(u8, rc.read_a(), &[_]u8{});
}
