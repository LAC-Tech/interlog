const std = @import("std");
const builtin = @import("builtin");
const testing = std.testing;
const Allocator = std.mem.Allocator;

const util = @import("util.zig");

const FixVec = util.FixVec;
const FixVecErr = util.FixVecErr;
const Region = util.Region;

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
    const id = ReplicaID.init(&rng);
    std.debug.print("{}", .{id});
}

const event = struct {
    pub const ID = struct { origin: ReplicaID, pos: usize };
    pub const Header = struct { byte_len: usize, id: ID };

    comptime {
        const header_size = @sizeOf(Header);
        if (header_size != 32) {
            @compileLog("header size = ", @sizeOf(Header));
            @compileError("event header sized has changed");
        }
    }

    pub const Event = struct {
        id: ID,
        payload: []const u8,

        pub fn onDiskSize(self: @This()) usize {
            return @sizeOf(Header) + self.payload.len;
        }
    };

    fn read(bytes: []const u8, offset: usize) ?Event {
        const header_region = util.Region.init(offset, @sizeOf(Header));
        const header_bytes: []u8 = header_region.read(bytes);
        const header: Header = @bitCast(header_bytes);

        const payload_region = header_region.rightAdjacent(header.byte_len);
        const payload = try payload_region.read(bytes);

        return .{ .id = header.id, .payload = payload };
    }

    fn append(buf: *FixVec(u8), e: *const Event) FixVecErr!void {
        const header_region = Region.init(buf.len, @sizeOf(event.Header));
        const header = .{ .byte_len = e.payload.len, .id = e.id };
        const payload_region = Region.init(header_region.end(), e.payload.len);
        const next_offset = buf.len + e.onDiskSize();
        try buf.resize(next_offset);
        const header_bytes: []const u8 = @bitCast(header);

        header_region.write(buf, header_bytes);
        payload_region.write(buf, event.payload);
    }

    const Iterator = struct {
        bytes: []const u8,
        index: usize,

        pub fn init(bytes: []const u8) @This() {
            return .{ .bytes = bytes, .index = 0 };
        }

        pub fn next(self: @This()) ?Event {
            const result = try read(self.bytes, self.index);
            self.index += result.on_disk_size();
            return result;
        }
    };
};

test "let's write some bytes" {
    var bytes = try FixVec(u8).init(testing.allocator, 20);

    var rng = std.rand.DefaultPrng.init(rng_seed());
    const id = ReplicaID.init(&rng);

    const evt = .{ .id = .{ .origin = id, .pos = 0 }, .payload = "j;fkls" };

    try event.append(&bytes, &evt);

    var it = event.Iterator.init(bytes.asSlice());

    while (it.next()) |e| {
        try testing.expectEqualSlices(u8, evt.payload, e.payload);
    }

    try testing.expectEqualDeep(event.read(bytes.asSlice(), 8), *evt);
    try testing.expectEqual(1, 2);
}

const TxnWriteBuf = struct {
    bytes: FixVec(u8),

    fn init(capacity: usize) @This() {
        return .{ .bytes = FixVec(u8).init(capacity) };
    }
};

const ReadCache = struct {
    mem: []u8,
    logical_start: usize,
    a: Region,
    b: Region,

    pub fn init(allocator: Allocator, capacity: usize) !@This() {
        return ReadCache{
            .mem = try allocator.alloc(u8, capacity),
            .logical_start = 0,
            .a = Region.zero(),
            .b = Region.zero(),
        };
    }

    pub fn deinit(self: *@This(), allocator: Allocator) void {
        allocator.free(self.mem);
        self.* = undefined;
    }

    pub fn update(self: @This(), txn_write_buf: []const u8) void {
        const a_would_overflow = self.a.end + txn_write_buf.len + self.mem.len;

        if (!a_would_overflow and self.b.end == 0) {
            self.write_a(txn_write_buf);
            return;
        }

        const a_will_be_modified = self.b.end + txn_write_buf.len >= self.a.pos;

        if (!a_will_be_modified) {
            self.write_b(txn_write_buf);
            return;
        }

        const b_would_overflow = self.b.end + txn_write_buf.len > self.mem.len;

        if (b_would_overflow) {
            self.a = Region(u8).init(0, self.b.end);
            self.b = Region(u8).zero();
        }

        if (self.new_a_byte_pos(txn_write_buf)) |new_a_pos| {
            self.a.change_pos(new_a_pos);
            self.write_b(txn_write_buf);
        } else {
            self.a = Region(u8).init(0, self.b.end);
            self.b = Region(u8).zero();
            self.write_a(txn_write_buf);
        }
    }

    fn set_logical_start(self: *@This(), es: []const u8) void {
        self.logical_start = event.read(es, 0).id.pos;
    }

    fn new_a_byte_pos(self: @This(), es: []const u8) ?usize {
        const new_b_end = self.b.end + es.len;

        // return inside the loop once you find it, otherwise return null
        var offset: usize = 0;

        for (self.read_a()) |e| {
            offset += e.onDiskSize();

            if (offset > new_b_end) {
                return offset;
            }
        }

        return null;
    }

    fn write_a(self: *@This(), es: []const u8) void {
        Region.init(self.a.len, es.len).write(self.mem, es);
    }

    fn write_b(self: *@This(), es: []const u8) void {
        Region.init(self.b.len, es.len).write(self.mem, es);
    }

    pub fn read_a(self: @This()) []const u8 {
        return self.a.read(u8, self.mem);
    }

    pub fn read_b(self: @This()) []const u8 {
        return self.b.read(u8, self.mem);
    }
};

test "alloc and free buf" {
    const Letter = enum(u256) {
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

    var ja = util.JaggedArray(Letter).init(testing.allocator);

    defer ja.deinit();
    try ja.append(&.{ .w, .h, .o });
    try ja.append(&.{ .i, .s });
    try ja.append(&.{ .t, .h, .i, .s });
    try ja.append(&.{ .d, .o, .i, .n });
    try ja.append(&.{ .t, .h, .i, .s });
    try ja.append(&.{ .s, .y, .n, .t, .h, .e, .t, .i, .c });
    try ja.append(&.{ .t, .y, .p, .e });
    try ja.append(&.{ .o, .f });
    try ja.append(&.{ .a, .l, .p, .h, .a });
    try ja.append(&.{ .b, .e, .t, .a });
    try ja.append(&.{ .p, .s, .y, .c, .h, .e, .d, .e, .l, .i, .c });
    try ja.append(&.{ .f, .u, .n, .k, .i, .n });

    var rc = try ReadCache.init(testing.allocator, 8);
    defer rc.deinit(testing.allocator);
    try testing.expectEqualSlices(u8, rc.read_a(), &.{});
    try testing.expectEqualSlices(u8, rc.read_b(), &.{});
}
