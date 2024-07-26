const std = @import("std");
const err = @import("./err.zig");
const extalloc = @import("./extalloc.zig");
const mem = @import("./mem.zig");

const Allocator = std.mem.Allocator;

pub fn Log(comptime Storage: type) type {
    return struct {
        addr: Addr,
        enqueued: Enqueued,
        committed: Committed(Storage),

        pub fn init(
            addr: Addr,
            storage: Storage,
            buffers: Buffers,
        ) @This() {
            return .{
                .addr = addr,
                .enqueued = Enqueued.init(buffers.enqueued),
                .committed = Committed(Storage).init(storage, buffers.committed),
            };
        }

        pub fn enqueue(self: *@This(), payload: []const u8) err.Write!void {
            const id = event.ID{
                .origin = self.addr,
                .logical_pos = self.enqueued.count() + self.committed.count(),
            };

            const e = event.Event{ .id = id, .payload = payload };

            try self.enqueued.append(&e);
        }

        /// Returns number of events committed
        pub fn commit(self: *@This()) err.Write!usize {
            const result = self.enqueued.offsets.len;
            try self.committed.append(
                self.enqueued.offsets.asSlice(),
                self.enqueued.events.asSlice(),
            );
            self.enqueued.reset(self.committed.lastOffset());
            return result;
        }

        pub fn rollback(self: *@This()) void {
            self.enqueued.reset(self.committed.last_offset());
        }

        pub fn readFromEnd(
            self: *@This(),
            n: usize,
            buf: *event.Buf,
        ) err.Write!void {
            try self.committed.readFromEnd(n, buf);
        }
    };
}

// TODO:
// "hey lewis… small trick for log buffers… never check for buffer overruns;
// mark a readonly page at the end of the buffer; the OS notifies you with an
// interrupt; result = writes with no checks / error checks i.e. no stalls for
// CPU code pipeline flushes because of branch mispredictions"
// - filasieno
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
    offsets: extalloc.Vec(usize),
    events: event.Buf,
    next_committed_offset: usize,
    pub fn init(buffers: Buffers.Enqueued) @This() {
        return .{
            .offsets = extalloc.Vec(usize).fromSlice(buffers.offsets),
            .events = event.Buf.fromSlice(buffers.events),
            .next_committed_offset = 0,
        };
    }

    pub fn append(self: *@This(), e: *const event.Event) err.Write!void {
        const last_enqueued_offset =
            self.offsets.last() orelse self.next_committed_offset;

        const offset = last_enqueued_offset + e.storedSize();
        try self.offsets.append(offset);
        try self.events.append(e);
    }

    pub fn reset(self: *@This(), next_committed_offset: usize) void {
        self.offsets.clear();
        self.events.clear();
        self.next_committed_offset = next_committed_offset;
    }

    pub fn count(self: *@This()) usize {
        return self.offsets.len;
    }
};

fn Committed(comptime Storage: type) type {
    return struct {
        /// This is always one greater than the number of events stored; the last
        /// element is the next offset of the next event appended
        offsets: extalloc.Vec(usize),
        events: Storage,

        pub fn init(
            storage: Storage,
            buffers: Buffers.Committed,
        ) @This() {
            var offsets = extalloc.Vec(usize).fromSlice(buffers.offsets);
            // TODO: enforce this at the buffer level?
            offsets.append(0) catch unreachable;
            return .{ .offsets = offsets, .events = storage };
        }

        pub fn append(
            self: *@This(),
            offsets: []const usize,
            events: []const u8,
        ) err.Write!void {
            try self.offsets.appendSlice(offsets);
            try self.events.append(events);
        }

        pub fn lastOffset(self: *@This()) usize {
            return self.offsets.last() orelse unreachable;
        }

        pub fn count(self: @This()) usize {
            return self.offsets.len;
        }

        pub fn readFromEnd(
            self: @This(),
            n: usize,
            buf: *event.Buf,
        ) err.Write!void {
            buf.clear();
            const nTotalOffsets = self.offsets.len;
            const lastNOffsets = self.offsets.asSlice()[nTotalOffsets - n ..];

            for (lastNOffsets) |offset| {
                self.events.read(buf.asSlice(), offset);
            }
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

        pub fn storedSize(self: @This()) usize {
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

    fn append(
        buf: *extalloc.Vec(u8),
        e: *const Event,
    ) err.Write!void {
        const header_region = mem.Region.init(buf.len, @sizeOf(Header));
        const header = .{ .byte_len = e.payload.len, .id = e.id };
        const header_bytes = std.mem.asBytes(&header);

        const payload_region = mem.Region.init(
            header_region.end(),
            e.payload.len,
        );

        const new_buf_len = buf.len + e.storedSize();
        try buf.resize(new_buf_len);
        header_region.write(buf.asSlice(), header_bytes) catch unreachable;
        payload_region.write(buf.asSlice(), e.payload) catch unreachable;
    }

    // TODO: this should be the iterator for Buf I think
    const Iterator = struct {
        bytes: []const u8,
        index: usize,

        pub fn init(bytes: []const u8) @This() {
            return .{ .bytes = bytes, .index = 0 };
        }

        pub fn next(self: *@This()) ?Event {
            const result = read(self.bytes, self.index) orelse return null;
            self.index += result.storedSize();
            return result;
        }
    };

    const Buf = struct {
        bytes: extalloc.Vec(u8),

        fn fromSlice(bytes: []u8) @This() {
            return .{ .bytes = extalloc.Vec(u8).fromSlice(bytes) };
        }

        fn append(self: *@This(), e: *const event.Event) !void {
            try event.append(&self.bytes, e);
        }

        pub fn asSlice(self: @This()) []u8 {
            return self.bytes.asSlice();
        }

        pub fn clear(self: *@This()) void {
            self.bytes.clear();
        }
    };
};

test "let's write some bytes" {
    const bytes_buf = try std.testing.allocator.alloc(u8, 63);
    defer std.testing.allocator.free(bytes_buf);
    var bytes = extalloc.Vec(u8).fromSlice(bytes_buf);

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

const TestStorage = struct {
    buf: extalloc.Vec(u8),
    pub fn init(slice: []u8) @This() {
        return .{ .buf = extalloc.Vec(u8).fromSlice(slice) };
    }

    pub fn append(self: *@This(), data: []const u8) err.Write!void {
        try self.buf.appendSlice(data);
    }

    pub fn read(self: @This(), buf: []u8, offset: usize) void {
        @memcpy(buf, self.buf.asSlice()[offset .. offset + buf.len]);
    }
};

test "enqueue, commit and read data" {
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.Random.Pcg.init(seed);

    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const storage = TestStorage.init(try allocator.alloc(u8, 4096));
    const buffers = Buffers{
        .enqueued = .{
            .events = try allocator.alloc(u8, 4096),
            .offsets = try allocator.alloc(usize, 3),
        },
        .committed = .{
            .offsets = try allocator.alloc(usize, 3),
        },
    };
    var log = Log(TestStorage).init(
        Addr.init(std.Random.Pcg, &rng),
        storage,
        buffers,
    );

    var read_buf = event.Buf.fromSlice(try allocator.alloc(u8, 128));
    try log.enqueue("I have known the arcane law");
    try log.readFromEnd(1, &read_buf);
    try std.testing.expectEqual(try log.commit(), 1);
}
