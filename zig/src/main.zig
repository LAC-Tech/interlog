const builtin = @import("builtin");
const std = @import("std");

const assert = std.debug.assert;
const mem = std.mem;
const testing = std.testing;
const Allocator = std.mem.Allocator;

const util = @import("util.zig");

const FixVec = util.FixVec;
const FixVecAligned = util.FixVecAligned;
const Region = util.Region;

comptime {
    if (builtin.target.cpu.arch.endian() != .Little) {
        @compileError("Big Endian is not supported");
    }

    if (builtin.target.ptrBitWidth() != 64) {
        @compileError("This has only been tested on 64 bit machines");
    }
}

const LogID = enum(u128) {
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
    const id = LogID.init(&rng);
    _ = id;
}

const event = struct {
    pub const ID = packed struct { origin: LogID, logical_pos: usize };
    pub const Header = packed struct { byte_len: usize, id: ID };

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
            const result = @sizeOf(Header) + self.payload.len;
            // While payload sizes are arbitrary, on disk we want 8 byte alignment
            return (result + 7) & ~@as(u8, 7);
        }
    };

    fn read(bytes: []const u8, offset: usize) ?Event {
        const header_region = util.Region.init(offset, @sizeOf(Header));
        const header_bytes = header_region.read(u8, bytes) orelse return null;
        const header = mem.bytesAsValue(
            Header,
            header_bytes[0..@sizeOf(Header)],
        );

        const payload_region = Region.init(header_region.end(), header.byte_len);
        const payload = payload_region.read(u8, bytes) orelse return null;

        return .{ .id = header.id, .payload = payload };
    }

    fn append(buf: *FixVecAligned(u8, 8), e: *const Event) !void {
        const header_region = Region.init(buf.len, @sizeOf(Header));
        const header = .{ .byte_len = e.payload.len, .id = e.id };
        const header_bytes = mem.asBytes(&header);

        const payload_region = Region.init(header_region.end(), e.payload.len);

        const new_buf_len = buf.len + e.onDiskSize();
        try buf.resize(new_buf_len);
        header_region.write(buf.asSlice(), header_bytes) catch unreachable;
        payload_region.write(buf.asSlice(), e.payload) catch unreachable;
    }

    const Iterator = struct {
        bytes: []const u8,
        index: usize,

        pub fn init(bytes: []const u8) @This() {
            return .{ .bytes = bytes, .index = 0 };
        }

        pub fn next(self: *@This()) ?Event {
            const result = read(self.bytes, self.index) orelse return null;
            self.index += result.onDiskSize();
            return result;
        }
    };
};

test "let's write some bytes" {
    var bytes = try FixVecAligned(u8, 8).init(testing.allocator, 63);
    defer bytes.deinit(testing.allocator);

    var rng = std.rand.DefaultPrng.init(rng_seed());
    const id = LogID.init(&rng);

    const evt = .{
        .id = .{ .origin = id, .logical_pos = 0 },
        .payload = "j;fkls",
    };

    try event.append(&bytes, &evt);

    var it = event.Iterator.init(bytes.asSlice());

    while (it.next()) |e| {
        try testing.expectEqualSlices(u8, evt.payload, e.payload);
    }

    const actual = event.read(bytes.asSlice(), 0);

    try testing.expectEqualDeep(actual, evt);
}

const ReadCache = struct {
    mem: []align(8) u8,
    logical_start: usize,
    a: Region,
    b: Region,

    const UpdateErr = error{TooLongForCache};

    pub fn init(allocator: Allocator, capacity: usize) !@This() {
        return ReadCache{
            .mem = try allocator.alignedAlloc(u8, 8, capacity),
            .logical_start = 0,
            .a = Region.zero(),
            .b = Region.zero(),
        };
    }

    pub fn deinit(self: *@This(), allocator: Allocator) void {
        allocator.free(self.mem);
        self.* = undefined;
    }

    fn overlappingRegions(self: @This()) bool {
        return self.b.end() > self.a.pos;
    }

    pub fn update(self: *@This(), txn_write_buf: []align(8) const u8) !void {
        assert(!self.overlappingRegions());

        // Should never happen but... let's see if it does
        if (txn_write_buf.len > self.mem.len) {
            return error.TooLongForCache;
        }

        if (self.b.empty()) {
            if (self.a.empty()) {
                self.setLogicalStart(txn_write_buf);
            }

            self.tryAppendA(txn_write_buf) catch {
                // If there's no space, start B, write to that, and truncate A
                try self.tryAppendB(txn_write_buf);
            };
        } else {
            try self.tryAppendB(txn_write_buf);
        }

        assert(!self.overlappingRegions());
    }

    fn setLogicalStart(self: *@This(), es: []align(8) const u8) void {
        self.logical_start = event.read(es, 0).?.id.logical_pos;
    }

    fn newABytePos(self: @This(), es: []align(8) const u8) ?usize {
        const new_b_end = self.b.end() + es.len;

        // return inside the loop once you find it, otherwise return null
        var offset: usize = 0;

        var a_event_iter = event.Iterator.init(self.readA());
        while (a_event_iter.next()) |e| {
            offset += e.onDiskSize();

            if (offset > new_b_end) {
                return offset;
            }
        }

        return null;
    }

    fn tryAppendA(self: *@This(), es: []const u8) !void {
        try Region.init(self.a.len, es.len).write(self.mem, es);
        self.a.lengthen(es.len);
    }

    fn tryAppendB(self: *@This(), txn_write_buf: []align(8) const u8) !void {
        if (self.newABytePos(txn_write_buf)) |new_a_pos| {
            self.a.changePos(new_a_pos);
            try Region
                .init(self.b.len, txn_write_buf.len)
                .write(self.mem, txn_write_buf);
            self.b.lengthen(txn_write_buf.len);
        } else {
            // New a position would fragment memory across an event
            // B gets deleted and we move to a fresh A
            self.a = Region.init(0, txn_write_buf.len);
            self.setLogicalStart(txn_write_buf);
            try self.tryAppendA(txn_write_buf);
        }
    }

    pub fn readA(self: @This()) []const u8 {
        return self.a.read(u8, self.mem).?;
    }

    pub fn readB(self: @This()) []const u8 {
        return self.b.read(u8, self.mem).?;
    }
};

const Log = struct {
    id: LogID,
    txn_write_buf: FixVecAligned(u8, 8),
    read_cache: ReadCache,
    key_index: FixVec(usize),
    arena: std.heap.ArenaAllocator,

    const Config = struct {
        capacity: struct {
            txn_write_buf: usize,
            read_cache: usize,
            key_index: usize,
        },
    };

    fn init(
        child_allocator: mem.Allocator,
        rng: anytype,
        config: Config,
    ) !@This() {
        var arena = std.heap.ArenaAllocator.init(child_allocator);
        const allocator = arena.allocator();
        return .{
            .id = LogID.init(rng),
            .txn_write_buf = try FixVecAligned(u8, 8).init(
                allocator,
                config.capacity.txn_write_buf,
            ),
            .read_cache = try ReadCache.init(
                allocator,
                config.capacity.read_cache,
            ),
            .key_index = try FixVec(usize).init(
                allocator,
                config.capacity.key_index,
            ),
            .arena = arena,
        };
    }

    fn deinit(self: *@This()) void {
        self.arena.deinit();
        self.* = undefined;
    }

    fn enqueue(self: *@This(), payload: anytype) !void {
        const e: event.Event = .{
            .id = .{ .origin = self.id, .logical_pos = self.key_index.len },
            .payload = mem.sliceAsBytes(payload),
        };

        try event.append(&self.txn_write_buf, &e);
    }

    fn commit(self: *@This()) !void {
        try self.read_cache.update(self.txn_write_buf.asSlice());
        self.txn_write_buf.clear();
    }
};

// TODO: I need ASCII diagram of my graph paper sketch, otherwise this test
// makes zero sense
test "test read cache" {
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

    //var ja = util.JaggedArray(Letter).init(testing.allocator);
    //defer ja.deinit();
    //try ja.append(&.{ .w, .h, .o });
    //try ja.append(&.{ .i, .s });
    //try ja.append(&.{ .t, .h, .i, .s });
    //try ja.append(&.{ .d, .o, .i, .n });
    //try ja.append(&.{ .t, .h, .i, .s });
    //try ja.append(&.{ .s, .y, .n, .t, .h, .e, .t, .i, .c });
    //try ja.append(&.{ .t, .y, .p, .e });
    //try ja.append(&.{ .o, .f });
    //try ja.append(&.{ .a, .l, .p, .h, .a });
    //try ja.append(&.{ .b, .e, .t, .a });
    //try ja.append(&.{ .p, .s, .y, .c, .h, .e, .d, .e, .l, .i, .c });
    //try ja.append(&.{ .f, .u, .n, .k, .i, .n });

    var log = try Log.init(
        testing.allocator,
        &rng,
        .{
            .capacity = .{
                .txn_write_buf = 512,
                .read_cache = 16 * 32,
                .key_index = 0x10000,
            },
        },
    );
    defer log.deinit();

    try log.enqueue(&[_]Letter{ .w, .h, .o });
    try log.commit();
    try testing.expectEqualDeep(Region.init(0, 4 * 32), log.read_cache.a);

    try log.enqueue(&[_]Letter{ .i, .s });
    try log.commit();
    try testing.expectEqualDeep(Region.init(0, 7 * 32), log.read_cache.a);

    try log.enqueue(&[_]Letter{ .t, .h, .i, .s });
    try log.commit();
    try testing.expectEqualDeep(Region.init(0, 12 * 32), log.read_cache.a);

    try log.enqueue(&[_]Letter{ .d, .o, .i, .n });
    try log.commit();
    try testing.expectEqualDeep(Region.init(7 * 32, 5 * 32), log.read_cache.a);
}
