const std = @import("std");
const mem = @import("./mem.zig");

const Allocator = std.mem.Allocator;

pub fn Log(comptime Storage: type) type {
    return struct {
        addr: Addr,
        enqueued: Enqueued,
        committed: Committed(Storage),

        pub fn init(
            addr: Addr,
            buffers: Buffers,
        ) Allocator.Error!@This() {
            return .{
                .addr = addr,
                .enqueued = Enqueued.init(buffers.enqueued),
                .committed = try Committed(Storage).init(buffers.committed),
            };
        }

        pub fn recv(self: *@This(), msg: Msg) void {
            _ = self;
            _ = msg;
            @panic("TODO");
        }

        pub fn enqueue(self: *@This(), payload: []const u8) void {
            _ = self;
            _ = payload;
            @panic("TODO");
        }

        pub fn commit(self: *@This()) void {
            _ = self;
            @panic("TODO");
        }
    };
}

const Buffers = struct {
    const Committed = struct {
        offsets: []usize,
    };
    const Enqueued = struct {
        offsets: []usize,
        events: []u8,
    };
    committed: @This().Committed,
    enqueued: @This().Enqueued,
};

const Enqueued = struct {
    offsets: FixVec(usize),
    events: event.Buf,
    next_committed_offset: usize,
    pub fn init(buffers: Buffers.Enqueued) @This() {
        return .{
            .offsets = FixVec(usize).fromSlice(buffers.offsets),
            .events = event.Buf.fromSlice(buffers.events),
            .next_committed_offset = 0,
        };
    }

    pub fn append(self: *@This(), e: &event.Event) {
    }
};

fn Committed(comptime Storage: type) type {
    return struct {
        offsets: FixVec(usize),
        events: Storage,

        pub fn init(
            buffers: Buffers.Committed,
        ) Allocator.Error!@This() {
            return .{
                .offsets = FixVec(usize).fromSlice(buffers.offsets),
                .events = Storage.init(),
            };
        }
    };
}

const event = struct {
    pub const ID = extern struct { origin: Addr, logical_pos: usize };
    pub const Header = extern struct { byte_len: usize, id: ID };

    comptime {
        if (@sizeOf(ID) != 24) {
            @compileLog("id size = ", @sizeOf(ID));
            @compileError("event ID sized has changed");
        }
        if (@sizeOf(Header) != 32) {
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
        const header_region = mem.Region.init(offset, @sizeOf(Header));
        const header_bytes = header_region.read(u8, bytes) orelse return null;
        const header = std.mem.bytesAsValue(
            Header,
            header_bytes[0..@sizeOf(Header)],
        );

        const payload_region = mem.Region.init(
            header_region.end(),
            header.byte_len,
        );
        const payload = payload_region.read(u8, bytes) orelse return null;

        return .{ .id = header.id, .payload = payload };
    }

    fn append(buf: *FixVecAligned(u8, 8), e: *const Event) void {
        const header_region = mem.Region.init(buf.len, @sizeOf(Header));
        const header = .{ .byte_len = e.payload.len, .id = e.id };
        const header_bytes = std.mem.asBytes(&header);

        const payload_region = mem.Region.init(
            header_region.end(),
            e.payload.len,
        );

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

    const Buf = struct {
        bytes: FixVec(u8),

        fn fromSlice(bytes: []u8) @This() {
            return .{ .bytes = FixVec(u8).fromSlice(bytes) };
        }

        fn push(self: *@This(), e: event.Event) !void {
            try append(self.bytes, e);
        }
    };
};

test "let's write some bytes" {
    const bytes_buf = try std.testing.allocator.alignedAlloc(u8, 8, 63);
    defer std.testing.allocator.free(bytes_buf);
    var bytes = FixVecAligned(u8, 8).fromSlice(bytes_buf);

    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.Random.Pcg.init(seed);
    const id = Addr.init(std.Random.Pcg, &rng);

    const evt = event.Event{
        .id = .{ .origin = id, .logical_pos = 0 },
        .payload = "j;fkls",
    };

    try event.append(&bytes, &evt);

    var it = event.Iterator.init(bytes.asSlice());

    while (it.next()) |e| {
        try std.testing.expectEqualSlices(u8, evt.payload, e.payload);
    }

    const actual = event.read(bytes.asSlice(), 0);

    try std.testing.expectEqualDeep(actual, evt);
}

pub fn FixVecAligned(comptime T: type, comptime alignment: ?u29) type {
    return struct {
        _items: Slice,
        len: usize,

        pub const Slice = if (alignment) |a| ([]align(a) T) else []T;

        pub const Err = error{Overflow};

        pub fn capacity(self: *@This()) usize {
            return self._items.len;
        }

        pub fn clear(self: *@This()) void {
            self.len = 0;
        }

        fn checkCapacity(self: *@This(), new_len: usize) Err!void {
            if (new_len > self.capacity()) {
                return Err.Overflow;
            }
        }

        pub fn resize(self: *@This(), new_len: usize) Err!void {
            try self.checkCapacity(new_len);
            self.len = new_len;
        }

        pub fn append(self: *@This(), item: T) Err!void {
            try self.checkCapacity(self.len + 1);
            self._items[self.len] = item;
            self.len += 1;
        }

        pub fn appendSlice(self: *@This(), items: []const T) Err!void {
            try self.checkCapacity(self.len + items.len);
            @memcpy(self._items[self.len .. self.len + items.len], items);
            self.len += items.len;
        }

        pub fn asSlice(self: @This()) Slice {
            return self._items[0..self.len];
        }

        pub fn fromSlice(slice: Slice) @This() {
            return .{ ._items = slice, .len = 0 };
        }
    };
}

pub fn FixVec(comptime T: type) type {
    return FixVecAligned(T, null);
}

test "fixvec stuff" {
    const buf = try std.testing.allocator.alloc(u64, 8);
    defer std.testing.allocator.free(buf);
    var fv = FixVec(u64).fromSlice(buf);

    try std.testing.expectEqual(fv.capacity(), 8);
    try fv.append(42);
    try std.testing.expectEqual(fv.len, 1);

    try fv.appendSlice(&[_]u64{ 6, 1, 9 });
    try std.testing.expectEqual(fv.len, 4);
}

pub const Addr = extern struct {
    word_a: u64,
    word_b: u64,
    pub fn init(comptime R: type, rng: *R) @This() {
        return .{
            .word_a = rng.random().int(u64),
            .word_b = rng.random().int(u64),
        };
    }

    pub fn format(
        self: @This(),
        comptime fmt: []const u8,
        options: std.fmt.FormatOptions,
        writer: anytype,
    ) !void {
        _ = fmt;
        _ = options;

        try writer.print("{x}{x}", .{ self.word_a, self.word_b });
    }

    const zero = Addr{ .words = .{ 0, 0 } };
};

comptime {
    const alignment = @alignOf(Addr);
    std.debug.assert(alignment == 8);
}

const Msg = struct {
    const Inner = union(enum) { sync_res: []event.Event };

    inner: Inner,
    origin: Addr,
};
