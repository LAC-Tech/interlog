const std = @import("std");
const util = @import("./util.zig");
const err = @import("./err.zig");

const mem = std.mem;
const assert = std.debug.assert;
const Allocator = std.mem.Allocator;
const ByteVec = std.ArrayListUnmanaged(u8);

pub fn Log(comptime Storage: type) type {
    return struct {
        addr: Addr,
        enqueued: Enqueued,
        committed: Committed(Storage),

        pub fn init(
            addr: Addr,
            storage: Storage,
            heap_memory: HeapMemory,
        ) @This() {
            return .{
                .addr = addr,
                .enqueued = Enqueued.init(heap_memory.enqueued),
                .committed = Committed(Storage).init(
                    storage,
                    heap_memory.committed,
                ),
            };
        }

        pub fn enqueue(self: *@This(), payload: []const u8) void {
            const id = event.ID{
                .origin = self.addr,
                .logical_pos = self.enqueued.count() + self.committed.count(),
            };

            const e = event.Event{ .id = id, .payload = payload };

            self.enqueued.append(&e);
        }

        /// Returns number of events committed
        pub fn commit(self: *@This()) usize {
            const result = self.enqueued.offsets.eventCount();
            self.committed.append(
                self.enqueued.offsets.tail(),
                self.enqueued.events.asSlice(),
            );
            self.enqueued.reset();
            return result;
        }

        pub fn rollback(self: *@This()) void {
            self.enqueued.reset(self.committed.last_offset());
        }

        // Returns an error because too small a read buffer should not crash
        pub fn readFromEnd(
            self: *@This(),
            n: usize,
            buf: *event.Buf,
        ) err.ReadBuf!void {
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
const HeapMemory = struct {
    const Committed = struct {
        offsets: []StorageOffset,
    };
    const Enqueued = struct {
        offsets: []StorageOffset,
        events: []u8,
    };
    committed: @This().Committed,
    enqueued: @This().Enqueued,
};

const Enqueued = struct {
    offsets: StorageOffsets,
    events: event.Buf,
    pub fn init(buffers: HeapMemory.Enqueued) @This() {
        return .{
            .offsets = StorageOffsets.init(buffers.offsets, 0),
            .events = event.Buf.init(buffers.events),
        };
    }

    pub fn append(self: *@This(), e: *const event.Event) void {
        const offset = self.offsets.last().next(e);
        self.offsets.append(offset);
        self.events.append(e);
        assert(self.events.read(self.offsets.eventCount()) != null);
    }

    pub fn reset(self: *@This()) void {
        // By definiton, the last committed event is the last thing in the
        // eqneued buffer before reseting, which happens after committing
        const last_committed_event = self.offsets.last();
        self.offsets.reset(last_committed_event);
        self.events.clear();
    }

    pub fn count(self: *@This()) usize {
        return self.offsets.eventCount();
    }
};

fn Committed(comptime Storage: type) type {
    return struct {
        /// This is always one greater than the number of events stored; the last
        /// element is the next offset of the next event appended
        offsets: StorageOffsets,
        events: Storage,

        fn init(
            storage: Storage,
            buffers: HeapMemory.Committed,
        ) @This() {
            return .{
                .offsets = StorageOffsets.init(buffers.offsets, 0),
                .events = storage,
            };
        }

        fn append(
            self: *@This(),
            offsets: []const StorageOffset,
            events: []const u8,
        ) void {
            self.offsets.appendSlice(offsets);
            self.events.append(events);
        }

        fn lastOffset(self: *@This()) usize {
            return self.offsets.last();
        }

        fn count(self: @This()) usize {
            return self.offsets.eventCount();
        }

        fn readFromEnd(
            self: @This(),
            n: usize,
            buf: *event.Buf,
        ) err.ReadBuf!void {
            buf.clear();
            var offsets = self.offsets.asSlice();
            offsets = offsets[offsets.len - 1 - n ..];

            const size = offsets[offsets.len - 1].n - offsets[0].n;
            buf.resize(size);
            try self.events.read(&buf.bytes, offsets[0].n);
        }
    };
}

/// Maps a logical position (nth event) to a byte offset in storage
const StorageOffsets = struct {
    const Vec = std.ArrayListUnmanaged(StorageOffset);
    // Vec with some invariants:
    // - always at least one element: next offset, for calculating size
    offsets: Vec,
    fn init(buf: []StorageOffset, next_committed_offset: usize) @This() {
        var offsets = Vec.initBuffer(buf);
        offsets.appendAssumeCapacity(StorageOffset.init(next_committed_offset));
        return .{ .offsets = offsets };
    }

    fn reset(self: *@This(), next_committed_offset: StorageOffset) void {
        self.offsets.clearRetainingCapacity();
        self.offsets.appendAssumeCapacity(next_committed_offset);
    }

    fn tail(self: @This()) []StorageOffset {
        return self.offsets.items[1..];
    }

    fn asSlice(self: @This()) []StorageOffset {
        return self.offsets.items;
    }

    fn eventCount(self: @This()) usize {
        return self.offsets.items.len - 1;
    }

    fn last(self: @This()) StorageOffset {
        return self.offsets.getLast();
    }

    fn append(self: *@This(), offset: StorageOffset) void {
        assert(offset.n > self.last().n);
        self.offsets.appendAssumeCapacity(offset);
    }

    fn appendSlice(self: *@This(), slice: []const StorageOffset) void {
        for (slice) |offset| {
            self.offsets.appendAssumeCapacity(offset);
        }
    }

    fn get(self: @This(), index: usize) usize {
        return self.offsets.asSlice()[index];
    }
};
/// Q - why bother with with this seperate type?
/// A - because I actually found a bug because when it was just a usize
pub const StorageOffset = packed struct(usize) {
    const zero = @This().init(0);
    n: usize,
    fn init(n: usize) @This() {
        // All storage offsets must be 8 byte aligned
        assert(n % 8 == 0);
        return .{ .n = n };
    }
    fn next(self: @This(), e: *const event.Event) @This() {
        const result = @sizeOf(event.Header) + e.payload.len;
        // While payload sizes are arbitrary, on disk we want 8 byte alignment
        const stored_size = (result + 7) & ~@as(u8, 7);
        return @This().init(self.n + stored_size);
    }
};

const event = struct {
    pub const ID = extern struct { origin: Addr, logical_pos: usize };
    pub const Header = extern struct { payload_len: usize, id: ID };

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

        //pub fn storedSize(self: @This()) usize {
        //    const result = @sizeOf(Header) + self.payload.len;
        //    // While payload sizes are arbitrary, on disk we want 8 byte alignment
        //    return (result + 7) & ~@as(u8, 7);
        //}
    };

    fn read(bytes: []const u8, offset: StorageOffset) ?Event {
        const header_end = offset.n + @sizeOf(Header);
        const header = std.mem.bytesAsValue(Header, bytes[offset.n..header_end]);
        const payload_end = header_end + header.payload_len;
        const payload = bytes[header_end..payload_end];

        return .{ .id = header.id, .payload = payload };
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
        bytes: ByteVec,

        fn init(buf: []u8) @This() {
            return .{ .bytes = ByteVec.initBuffer(buf) };
        }

        fn append(self: *@This(), e: *const event.Event) void {
            const header = .{ .byte_len = e.payload.len, .id = e.id };
            const header_bytes: []const u8 = std.mem.asBytes(&header);
            self.bytes.appendSliceAssumeCapacity(header_bytes);
            self.bytes.appendSliceAssumeCapacity(e.payload);
        }

        fn read(self: @This(), offset: StorageOffset) ?Event {
            return event.read(self.bytes.items, offset);
        }

        fn asSlice(self: @This()) []u8 {
            return self.bytes.items;
        }

        fn clear(self: *@This()) void {
            self.bytes.clearRetainingCapacity();
        }

        fn resize(self: *@This(), new_len: usize) void {
            self.bytes.items.len = new_len;
        }
    };
};

//test "let's write some bytes" {
//    const bytes_buf = try std.testing.allocator.alloc(u8, 63);
//    defer std.testing.allocator.free(bytes_buf);
//    var buf = event.Buf.init(bytes_buf);
//
//    const seed: u64 = std.crypto.random.int(u64);
//    var rng = std.Random.Pcg.init(seed);
//    const id = Addr.init(std.Random.Pcg, &rng);
//
//    const evt = event.Event{
//        .id = .{ .origin = id, .logical_pos = 0 },
//        .payload = "j;fkls",
//    };
//
//    buf.append(&evt);
//    var it = event.Iterator.init(buf.asSlice());
//
//    while (it.next()) |e| {
//        try std.testing.expectEqualSlices(u8, evt.payload, e.payload);
//    }
//
//    const actual = buf.read(0);
//
//    try std.testing.expectEqualDeep(actual, evt);
//}

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
    assert(alignment == 8);
}

const Msg = struct {
    const Inner = union(enum) { sync_res: []event.Event };

    inner: Inner,
    origin: Addr,
};

const TestStorage = struct {
    bytes: ByteVec,
    pub fn init(buf: []u8) @This() {
        return .{ .bytes = ByteVec.initBuffer(buf) };
    }

    pub fn append(self: *@This(), data: []const u8) void {
        self.bytes.appendSliceAssumeCapacity(data);
    }

    pub fn read(
        self: @This(),
        dest: *ByteVec,
        offset: usize,
    ) err.ReadBuf!void {
        // TODO: bounds check
        const requested = self.bytes.items[offset .. offset + dest.items.len];
        dest.appendSliceAssumeCapacity(requested);
    }
};

test "enqueue, commit and read data" {
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.Random.Pcg.init(seed);

    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const addr = Addr.init(std.Random.Pcg, &rng);
    const storage = TestStorage.init(try allocator.alloc(u8, 4096));
    const heap_memory = HeapMemory{
        .enqueued = .{
            .events = try allocator.alloc(u8, 4096),
            .offsets = try allocator.alloc(StorageOffset, 3),
        },
        .committed = .{
            .offsets = try allocator.alloc(StorageOffset, 3),
        },
    };

    var log = Log(TestStorage).init(addr, storage, heap_memory);

    var read_buf = event.Buf.init(try allocator.alloc(u8, 128));
    log.enqueue("I have known the arcane law");
    try std.testing.expectEqual(log.commit(), 1);
    try log.readFromEnd(1, &read_buf);

    const actual = read_buf.read(StorageOffset.zero);

    try std.testing.expectEqualSlices(
        u8,
        "I have known the arcane law",
        actual.?.payload,
    );
}
