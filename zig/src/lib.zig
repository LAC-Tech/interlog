const std = @import("std");
const err = @import("./err.zig");

const mem = std.mem;
const testing = std.testing;
const assert = std.debug.assert;
const Allocator = std.mem.Allocator;
const ArrayListUnmanaged = std.ArrayListUnmanaged;

fn vecFromBuf(comptime T: type, buf: []T) ArrayListUnmanaged(T) {
    return ArrayListUnmanaged(T).initBuffer(buf);
}

pub fn Log(comptime Storage: type) type {
    return struct {
        addr: Addr,
        enqd: Enqueued,
        cmtd: Committed(Storage),
        /// Acquaintances.
        /// Addrs the Log has interacted with.
        /// Storing them here allows us to reference them with a u16 ptr inside
        /// a committed event, shortening the header
        as: ArrayListUnmanaged(Addr),

        pub fn init(
            addr: Addr,
            storage: Storage,
            heap_memory: HeapMemory,
        ) @This() {
            var as = vecFromBuf(Addr, heap_memory.acquaintances);
            as.appendAssumeCapacity(addr);
            return .{
                .addr = addr,
                .enqd = Enqueued.init(heap_memory.enqueued, &as.items),
                .cmtd = Committed(Storage).init(
                    storage,
                    heap_memory.committed,
                ),
                .as = as,
            };
        }

        /// Returns bytes enqueued
        pub fn enqueue(self: *@This(), payload: []const u8) u64 {
            const id = Event.ID{
                .origin = self.addr,
                .logical_pos = self.enqd.count() + self.cmtd.count(),
            };

            const e = Event{ .id = id, .payload = payload };
            return self.enqd.append(&e);
        }

        /// Returns number of events committed
        pub fn commit(self: *@This()) u64 {
            const txn = self.enqd.txn();
            const result = txn.offsets.len;
            self.cmtd.append(txn.offsets, txn.events);
            self.enqd.reset();
            return result;
        }

        pub fn rollback(self: *@This()) void {
            self.enqd.reset(self.cmtd.last_offset());
        }

        // Returns an error because too small a read buffer should not crash
        pub fn readFromEnd(
            self: *@This(),
            n: u64,
            buf: *ReadBuf,
        ) err.ReadBuf!void {
            try self.cmtd.readFromEnd(n, buf);
        }
    };
}

// TODO:
// "hey lewis… small trick for log buffers… never check for buffer overruns;
// mark a readonly page at the end of the buffer; the OS notifies you with an
// interrupt; result = writes with no checks / error checks i.e. no stalls for
// CPU code pipeline flushes because of branch mispredictions"
// - filasieno
pub const HeapMemory = struct {
    const Committed = struct {
        offsets: []StorageOffset,
    };
    const Enqueued = struct {
        offsets: []StorageOffset,
        events: []u8,
    };
    committed: @This().Committed,
    enqueued: @This().Enqueued,
    acquaintances: *[std.math.maxInt(u16)]Addr,
};

const Enqueued = struct {
    const Transaction = struct {
        offsets: []const StorageOffset,
        events: []const u8,
    };
    offsets: StorageOffsets,
    events: ArrayListUnmanaged(u8),
    /// Committed Acquaintances
    cas: *[]const Addr,
    fn init(buffers: HeapMemory.Enqueued, cas: *[]const Addr) @This() {
        return .{
            .offsets = StorageOffsets.init(buffers.offsets, 0),
            .events = vecFromBuf(u8, buffers.events),
            .cas = cas,
        };
    }

    /// Returns bytes enqueued
    fn append(self: *@This(), e: *const Event) u64 {
        const offset = self.offsets.last().next(e);
        self.offsets.append(offset);
        const header = .{ .id = e.id, .payload_len = e.payload.len };
        e.appendTo(Event.LongHeader, header, &self.events);
        return self.events.items.len;
    }

    fn reset(self: *@This()) void {
        // By definiton, the last committed event is the last thing in the
        // eqneued buffer before reseting, which happens after committing
        const last_committed_event = self.offsets.last();
        self.offsets.reset(last_committed_event);
        self.events.clearRetainingCapacity();
    }

    fn count(self: *@This()) u64 {
        return self.offsets.eventCount();
    }

    /// Returns all relevant data to be committed
    fn txn(self: @This()) Transaction {
        const size_in_bytes = self.offsets.sizeSpanned();

        return .{
            .offsets = self.offsets.tail(),
            .events = self.events.items[0..size_in_bytes],
        };
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

        fn lastOffset(self: *@This()) u64 {
            return self.offsets.last();
        }

        fn count(self: @This()) u64 {
            return self.offsets.eventCount();
        }

        fn readFromEnd(
            self: @This(),
            n: u64,
            buf: *ReadBuf,
        ) err.ReadBuf!void {
            buf.clear();
            var offsets = self.offsets.asSlice();
            offsets = offsets[offsets.len - 1 - n ..];

            const size = offsets[offsets.len - 1].n - offsets[0].n;
            try self.events.read(buf.resize(size, n), offsets[0].n);
        }
    };
}

/// Maps a logical position (nth event) to a byte offset in storage
const StorageOffsets = struct {
    const Vec = std.ArrayListUnmanaged(StorageOffset);
    // Vec with some invariants:
    // - always at least one element: next offset, for calculating size
    vec: Vec,
    fn init(buf: []StorageOffset, next_committed_offset: u64) @This() {
        var vec = Vec.initBuffer(buf);
        vec.appendAssumeCapacity(StorageOffset.init(next_committed_offset));
        return .{ .vec = vec };
    }

    fn reset(self: *@This(), next_committed_offset: StorageOffset) void {
        self.vec.clearRetainingCapacity();
        self.vec.appendAssumeCapacity(next_committed_offset);
    }

    fn tail(self: @This()) []StorageOffset {
        return self.vec.items[1..];
    }

    fn asSlice(self: @This()) []StorageOffset {
        return self.vec.items;
    }

    fn eventCount(self: @This()) u64 {
        return self.vec.items.len - 1;
    }

    fn last(self: @This()) StorageOffset {
        return self.vec.getLast();
    }

    fn append(self: *@This(), offset: StorageOffset) void {
        assert(offset.n > self.last().n);
        self.vec.appendAssumeCapacity(offset);
    }

    fn appendSlice(self: *@This(), slice: []const StorageOffset) void {
        for (slice) |offset| {
            self.vec.appendAssumeCapacity(offset);
        }
    }

    fn get(self: @This(), index: u64) u64 {
        return self.vec.asSlice()[index];
    }

    fn sizeSpanned(self: @This()) u64 {
        return self.vec.getLast().n - self.vec.items[0].n;
    }
};

/// Q - why bother with with this seperate type?
/// A - because I actually found a bug because when it was just a usize
pub const StorageOffset = packed struct(u64) {
    pub const zero = @This().init(0);

    n: u64,
    fn init(n: u64) @This() {
        // All storage offsets must be 8 byte aligned
        assert(n % 8 == 0);
        return .{ .n = n };
    }

    fn next(self: @This(), e: *const Event) @This() {
        return @This().init(self.n + e.storedSize());
    }
};

const Event = struct {
    pub const ID = extern struct { origin: Addr, logical_pos: u64 };

    // Stand alone, self describing header
    pub const LongHeader = extern struct { payload_len: u64, id: ID };

    // Header that points into other parts of the log
    pub const ShortHeader = packed struct(u64) {
        payload_len: u48,
        origin_ptr: u16,
    };

    comptime {
        assert(@sizeOf(ID) == 24);
        assert(@sizeOf(LongHeader) == 32);
        assert(@sizeOf(ShortHeader) == 8);
    }

    id: ID,
    payload: []const u8,

    /// 8 byte aligned size
    fn storedSize(self: @This()) u64 {
        const unaligned_size = @sizeOf(LongHeader) + self.payload.len;
        return (unaligned_size + 7) & ~@as(u8, 7);
    }

    fn appendTo(
        self: @This(),
        comptime Header: type,
        header: Header,
        byte_vec: *ArrayListUnmanaged(u8),
    ) void {
        const header_bytes: []const u8 = std.mem.asBytes(&header);
        const old_len = byte_vec.items.len;
        byte_vec.appendSliceAssumeCapacity(header_bytes);
        byte_vec.appendSliceAssumeCapacity(self.payload);
        byte_vec.items.len = old_len + self.storedSize();
    }

    fn read(bytes: []const u8, offset: StorageOffset) Event {
        const header_end = offset.n + @sizeOf(LongHeader);
        const header = std.mem.bytesAsValue(LongHeader, bytes[offset.n..header_end]);
        const payload_end = header_end + header.payload_len;
        const payload = bytes[header_end..payload_end];

        return .{ .id = header.id, .payload = payload };
    }
};

pub const ReadBuf = struct {
    const Iterator = struct {
        read_buf: *const ReadBuf,
        offset_index: StorageOffset,
        event_index: u64,

        fn init(read_buf: *const ReadBuf) @This() {
            return .{
                .read_buf = read_buf,
                .offset_index = StorageOffset.zero,
                .event_index = 0,
            };
        }

        pub fn next(self: *@This()) ?Event {
            if (self.event_index == self.read_buf.n_events) return null;
            const result = self.read_buf.read(self.offset_index);
            self.offset_index = self.offset_index.next(&result);
            self.event_index += 1;
            return result;
        }
    };

    bytes: ArrayListUnmanaged(u8),
    n_events: u64,

    pub fn init(buf: []u8) @This() {
        return .{ .bytes = vecFromBuf(u8, buf), .n_events = 0 };
    }

    fn append(self: *@This(), e: *const Event) void {
        const header = .{
            .id = e.id,
            .payload_len = e.payload.len,
        };
        e.appendTo(Event.LongHeader, header, &self.bytes);

        self.n_events += 1;
    }

    pub fn read(self: @This(), offset: StorageOffset) Event {
        return Event.read(self.bytes.items, offset);
    }

    fn clear(self: *@This()) void {
        self.bytes.clearRetainingCapacity();
        self.n_events = 0;
    }

    /// This is meant for pwrite type interfaces
    /// new area must be written to immediately.
    fn resize(self: *@This(), new_len: u64, num_new_events: u64) []u8 {
        self.n_events += num_new_events;
        return self.bytes.addManyAsSliceAssumeCapacity(new_len);
    }

    pub fn iter(self: *const @This()) Iterator {
        return Iterator.init(self);
    }
};

test "let's write some bytes" {
    const bytes_buf = try std.testing.allocator.alloc(u8, 127);
    defer std.testing.allocator.free(bytes_buf);
    var buf = ReadBuf.init(bytes_buf);

    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.Random.Pcg.init(seed);
    const id = Addr.init(std.Random.Pcg, &rng);

    const evt = Event{
        .id = .{ .origin = id, .logical_pos = 0 },
        .payload = "j;fkls",
    };

    buf.append(&evt);
    var it = buf.iter();

    while (it.next()) |e| {
        try std.testing.expectEqualSlices(u8, evt.payload, e.payload);
    }

    const actual = buf.read(StorageOffset.zero);

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
    assert(alignment == 8);
}

const Msg = struct {
    const Inner = union(enum) { sync_res: []Event };

    inner: Inner,
    origin: Addr,
};

pub const TestStorage = struct {
    bytes: ArrayListUnmanaged(u8),
    pub fn init(buf: []u8) @This() {
        return .{ .bytes = vecFromBuf(u8, buf) };
    }

    pub fn append(self: *@This(), data: []const u8) void {
        self.bytes.appendSliceAssumeCapacity(data);
    }

    pub fn read(
        self: @This(),
        dest: []u8,
        offset: u64,
    ) err.ReadBuf!void {
        // TODO: bounds check
        const requested = self.bytes.items[offset .. offset + dest.len];
        @memcpy(dest, requested);
    }
};

test "enqueue, commit and read data" {
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.Random.Pcg.init(seed);

    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const addr = Addr.init(std.Random.Pcg, &rng);
    const storage = TestStorage.init(try allocator.alloc(u8, 272));
    const heap_memory = .{
        .enqueued = .{
            .events = try allocator.alloc(u8, 136),
            .offsets = try allocator.alloc(StorageOffset, 3),
        },
        .committed = .{
            .offsets = try allocator.alloc(StorageOffset, 5),
        },
        .acquaintances = try allocator.create([std.math.maxInt(u16)]Addr),
    };

    var log = Log(TestStorage).init(addr, storage, heap_memory);

    var read_buf = ReadBuf.init(try allocator.alloc(u8, 136));
    try testing.expectEqual(64, log.enqueue("I have known the arcane law"));
    try testing.expectEqual(1, log.commit());
    try log.readFromEnd(1, &read_buf);
    try testing.expectEqualSlices(
        u8,
        "I have known the arcane law",
        read_buf.read(StorageOffset.zero).payload,
    );

    try testing.expectEqual(
        72,
        log.enqueue("On strange roads, such visions met"),
    );
    try testing.expectEqual(1, log.commit());
    try log.readFromEnd(1, &read_buf);
    var it = read_buf.iter();
    const next = it.next();
    const actual = next.?.payload;
    try testing.expectEqualSlices(
        u8,
        "On strange roads, such visions met",
        //read_buf.read(StorageOffset.zero).payload,
        actual,
    );

    // Read multiple things from the buffer
    try log.readFromEnd(2, &read_buf);
    it = read_buf.iter();

    try testing.expectEqualSlices(
        u8,
        "I have known the arcane law",
        it.next().?.payload,
    );

    try testing.expectEqualSlices(
        u8,
        "On strange roads, such visions met",
        it.next().?.payload,
    );

    // Bulk commit two things
    try testing.expectEqual(64, log.enqueue("That I have no fear, nor concern"));
    try testing.expectEqual(
        136,
        log.enqueue("For dangers and obstacles of this world"),
    );
    try testing.expectEqual(log.commit(), 2);

    try log.readFromEnd(2, &read_buf);
    it = read_buf.iter();

    try testing.expectEqualSlices(
        u8,
        "That I have no fear, nor concern",
        it.next().?.payload,
    );

    try testing.expectEqualSlices(
        u8,
        "For dangers and obstacles of this world",
        it.next().?.payload,
    );
}
