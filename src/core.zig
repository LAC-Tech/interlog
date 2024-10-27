//! Deterministic core of interlog.
//! Allows for different storage implementations.
//! - no memory allocations
//! - no libc

const std = @import("std");
const debug = std.debug;
const testing = std.testing;

// All errors, whether expected or unexpected, get an explicit err code.
// Why not use asserts?
// - they can't be caught, which I want to do for the inspector
// - poor dev experience, they don't tell me much in the console
// - an err union is one place where everything that can go wrong is documented
// - allows me to defer the decision on whether an err is recoverable or not
pub const Err = error{
    NotEnoughMem,
    StorageOffsetNonMonotonic,
    StorageOffsetNot8ByteAligned,
};

/// Making my own append only data structure
/// Main reason is so I can get a trappable error for buffer overflows
/// And the reason I want that, is for the simulator.
/// But also convenience - zigs arraylistunamanged is a mouthful
fn Vec(comptime T: type) type {
    return struct {
        mem: []T,
        len: usize,

        fn init(buf: []T) @This() {
            return .{ .mem = buf, .len = 0 };
        }

        fn push(self: *@This(), elem: T) error{NotEnoughMem}!void {
            if (self.len == self.mem.len) return error.NotEnoughMem;
            self.mem[self.len] = elem;
            self.len += 1;
        }

        fn pushSlice(
            self: *@This(),
            slice: []const T,
        ) error{NotEnoughMem}!void {
            const new_len = self.len + slice.len;
            if (new_len > self.mem.len) {
                return error.NotEnoughMem;
            }

            @memcpy(self.mem[self.len..new_len], slice);
            self.len = new_len;
        }

        fn peek(self: @This()) ?T {
            return if (self.len == 0) null else self.mem[self.len - 1];
        }

        fn clear(self: *@This()) void {
            self.len = 0;
        }

        fn asSlice(self: @This()) []const T {
            return self.mem[0..self.len];
        }
    };
}

fn alignTo8(unaligned: u64) u64 {
    return (unaligned + 7) & ~@as(u8, 7);
}

pub const Stats = struct {
    addr: Address,
    n_enqd_events: usize,
    n_cmtd_events: usize,
};

// TODO: explicitly list errors returned by each function
pub fn Log(comptime Storage: type) type {
    return struct {
        addr: Address,
        enqd: Enqueued,
        cmtd: Committed(Storage),

        pub fn init(
            addr: Address,
            storage: Storage,
            heap_mem: HeapMem,
        ) !@This() {
            const cmtd = try Committed(Storage).init(
                addr,
                storage,
                heap_mem.cmtd_acqs,
                heap_mem.cmtd_offsets,
            );

            const enqd = try Enqueued.init(
                heap_mem.enqd_offsets,
                heap_mem.enqd_events,
                &cmtd.acqs.asSlice(),
            );

            return .{ .addr = addr, .enqd = enqd, .cmtd = cmtd };
        }

        /// Returns accumulated number of bytes enqueued
        pub fn enqueue(self: *@This(), payload: []const u8) !u64 {
            const id = Event.ID{
                .origin = self.addr,
                .logical_pos = self.enqd.eventCount() + self.cmtd.eventCount(),
            };

            const e = Event{ .id = id, .payload = payload };
            return self.enqd.append(&e);
        }

        /// Returns number of events committed
        pub fn commit(self: *@This()) error{NotEnoughMem}!u64 {
            const txn = self.enqd.txn();
            const result = txn.offsets.len;
            try self.cmtd.append(txn.offsets, txn.events);
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
            buf: *Event.Buf,
        ) error{NotEnoughMem}!void {
            try self.cmtd.readFromEnd(n, buf);
        }

        pub fn stats(self: @This()) Stats {
            return .{
                .addr = self.addr,
                .n_cmtd_events = self.cmtd.eventCount(),
                .n_enqd_events = self.enqd.eventCount(),
            };
        }
    };
}

// Describes a region of some storage, whether primary or secondary .
const Region = struct {
    n_bytes: usize,
    offset: usize,

    fn readSlice(
        self: @This(),
        bytes: []const u8,
    ) error{NotEnoughMem}![]const u8 {
        const len = self.offset + self.n_bytes;
        return if (len > bytes.len) Err.NotEnoughMem else bytes[self.offset..len];
    }
};

// TODO:
// "hey lewis… small trick for log buffers… never check for buffer overruns;
// mark a readonly page at the end of the buffer; the OS notifies you with an
// interrupt; result = writes with no checks / error checks i.e. no stalls for
// CPU code pipeline flushes because of branch mispredictions"
// - filasieno
pub const HeapMem = struct {
    cmtd_offsets: []StorageOffset,
    cmtd_acqs: []Address,

    enqd_offsets: []StorageOffset,
    enqd_events: []u8,
};

// Staging area for events to be committed later
// This allows bulk put semantics, and also allows us to apply the headers.
const Enqueued = struct {
    const Transaction = struct {
        offsets: []const StorageOffset,
        events: []const u8,
    };
    offsets: StorageOffsets,
    events: Vec(u8),
    cmtd_acqs: *const []const Address,
    fn init(
        offset_buf: []StorageOffset,
        event_buf: []u8,
        cmtd_acqs: *const []const Address,
    ) !@This() {
        return .{
            .offsets = try StorageOffsets.init(offset_buf, 0),
            .events = Vec(u8).init(event_buf),
            .cmtd_acqs = cmtd_acqs,
        };
    }

    /// Returns accumulated number of bytes enqueued
    fn append(self: *@This(), e: *const Event) !u64 {
        const offset = try self.offsets.getLast().next(e);
        try self.offsets.append(offset);
        try e.appendTo(&self.events);
        return self.events.len;
    }

    fn reset(self: *@This()) void {
        // By definiton, the last committed event is the last thing in the
        // eqneued buffer before reseting, which happens after committing
        const last_committed_event = self.offsets.getLast();
        self.offsets.reset(last_committed_event);
        self.events.clear();
    }

    fn eventCount(self: @This()) u64 {
        return self.offsets.eventCount();
    }

    /// Returns all relevant data to be committed
    fn txn(self: @This()) Transaction {
        return .{
            .offsets = self.offsets.tail(),
            .events = self.events.asSlice(),
        };
    }
};

// Events that have already been committed, or persisted, to storage
fn Committed(comptime Storage: type) type {
    return struct {
        /// This is always one greater than the number of events stored; the last
        /// element is the next offset of the next event appended
        offsets: StorageOffsets,
        events: Storage,
        acqs: Acquaintances,

        fn init(
            addr: Address,
            storage: Storage,
            acqs_buf: []Address,
            offsets_buf: []StorageOffset,
        ) !@This() {
            var acqs = Acquaintances.init(acqs_buf);
            try acqs.append(addr);

            return .{
                .offsets = try StorageOffsets.init(offsets_buf, 0),
                .events = storage,
                .acqs = acqs,
            };
        }

        fn append(
            self: *@This(),
            offsets: []const StorageOffset,
            events: []const u8,
        ) !void {
            // TODO: these operations must be atomic
            try self.offsets.appendSlice(offsets);
            // TODO: If this fails, appending to offsets must be un-done
            try self.events.append(events);
        }

        fn lastOffset(self: *@This()) u64 {
            return self.offsets.getLast();
        }

        fn eventCount(self: @This()) u64 {
            return self.offsets.eventCount();
        }

        fn readFromEnd(
            self: @This(),
            n: u64,
            buf: *Event.Buf,
        ) error{NotEnoughMem}!void {
            buf.clear();
            const region = self.offsets.lastNEvents(n);
            try buf.appendFromStorage(Storage, self.events, region, n);
        }
    };
}

/// Maps a logical position (nth event) to a byte offset in storage
/// Wrapper around a Vec with some invariants:
/// - always at least one element: next offset, for calculating size
const StorageOffsets = struct {
    vec: Vec(StorageOffset),
    fn init(buf: []StorageOffset, next_committed_offset: u64) !@This() {
        var vec = Vec(StorageOffset).init(buf);
        const first = try StorageOffset.init(next_committed_offset);
        try vec.push(first);
        return .{ .vec = vec };
    }

    fn reset(self: *@This(), next_committed_offset: StorageOffset) void {
        self.vec.clear();
        self.vec.push(next_committed_offset) catch unreachable;
    }

    fn tail(self: @This()) []const StorageOffset {
        return self.vec.asSlice()[1..];
    }

    fn lastNEvents(self: @This(), n: usize) Region {
        const offsets = self.vec.asSlice();
        const last = offsets.len - 1;
        const first = last - n;
        const size = offsets[last].n - offsets[first].n;
        const region = .{ .n_bytes = size, .offset = offsets[first].n };
        return region;
    }

    fn eventCount(self: @This()) u64 {
        return self.vec.mem.len - 1;
    }

    fn getLast(self: @This()) StorageOffset {
        return self.vec.peek().?;
    }

    fn append(self: *@This(), offset: StorageOffset) !void {
        if (self.getLast().n >= offset.n) return Err.StorageOffsetNonMonotonic;
        try self.vec.push(offset);
    }

    fn appendSlice(self: *@This(), slice: []const StorageOffset) !void {
        for (slice) |offset| {
            try self.vec.push(offset);
        }
    }
};

/// Offser, in bytes, where an event is stored.
/// Q - why bother with with this seperate type?
/// A - because I actually found a bug because when it was just a usize
pub const StorageOffset = packed struct(u64) {
    pub const zero = .{ .n = 0 };

    n: u64,
    fn init(n: u64) Err!@This() {
        return if (n % 8 != 0) Err.StorageOffsetNot8ByteAligned else .{ .n = n };
    }

    fn next(self: @This(), e: *const Event) !@This() {
        const unpadded_size = @sizeOf(Event.Header) + e.payload.len;
        const size = alignTo8(unpadded_size);
        return @This().init(self.n + size);
    }
};

/// Addrs the Log has interacted with.
/// Storing them here allows us to reference them with a u16 ptr inside
/// a committed event, allowing shortening the header for storaage.
const Acquaintances = struct {
    vec: Vec(Address),
    fn init(buf: []Address) @This() {
        if (buf.len > std.math.maxInt(u16)) {
            @panic("Must be able to index acquaintances with a u16");
        }

        return .{ .vec = Vec(Address).init(buf) };
    }

    fn get(self: @This(), index: u16) Address {
        return self.vec.mem[index];
    }

    fn append(self: *@This(), addr: Address) !void {
        return self.vec.push(addr);
    }

    fn asSlice(self: @This()) []Address {
        return self.vec.mem;
    }
};

const Event = struct {
    pub const ID = extern struct { origin: Address, logical_pos: u64 };

    // Stand alone, self describing header
    // All info here is needed to rebuild the log from a binary file.
    pub const Header = extern struct { payload_len: u64, id: Event.ID };

    comptime {
        debug.assert(@sizeOf(ID) == 24);
        debug.assert(@sizeOf(Header) == 32);
    }

    id: ID,
    payload: []const u8,

    fn appendTo(
        self: @This(),
        byte_vec: *Vec(u8),
    ) !void {
        const header = Header{ .payload_len = self.payload.len, .id = self.id };
        const header_bytes: *align(8) const [32]u8 = std.mem.asBytes(&header);
        try byte_vec.pushSlice(header_bytes);
        try byte_vec.pushSlice(self.payload);
        byte_vec.len = alignTo8(byte_vec.len); // add padding
    }

    fn read(
        bytes: []const u8,
        offset: StorageOffset,
    ) Event {
        const header_end = offset.n + @sizeOf(Header);
        const header_bytes = bytes[offset.n..header_end];
        const header = std.mem.bytesAsValue(Header, header_bytes);
        const payload_end = header_end + header.payload_len;
        const payload = bytes[header_end..payload_end];

        return .{ .id = header.id, .payload = payload };
    }

    pub const Buf = struct {
        const Iterator = struct {
            events: *const Buf,
            offset_index: StorageOffset,
            event_index: u64,

            fn init(events: *const Buf) @This() {
                return .{
                    .events = events,
                    .offset_index = StorageOffset.zero,
                    .event_index = 0,
                };
            }

            pub fn next(
                self: *@This(),
            ) !?Event {
                if (self.event_index == self.events.n_events) return null;
                const result = self.events.read(self.offset_index);
                self.offset_index = try self.offset_index.next(&result);
                self.event_index += 1;
                return result;
            }
        };

        bytes: Vec(u8),
        n_events: u64,

        pub fn init(buf: []u8) @This() {
            return .{ .bytes = Vec(u8).init(buf), .n_events = 0 };
        }

        fn append(self: *@This(), evt: *const Event) !void {
            try evt.appendTo(&self.bytes);
            self.n_events += 1;
        }

        pub fn read(self: @This(), offset: StorageOffset) Event {
            return Event.read(self.bytes.mem, offset);
        }

        fn appendFromStorage(
            self: *@This(),
            comptime Storage: type,
            storage: Storage,
            region: Region,
            n_events: usize,
        ) !void {
            try storage.read(&self.bytes, region);
            self.n_events += n_events;
        }
        fn clear(self: *@This()) void {
            self.bytes.clear();
            self.n_events = 0;
        }

        pub fn iter(self: *const @This()) Iterator {
            return Iterator.init(self);
        }

        pub fn format(
            self: @This(),
            comptime fmt: []const u8,
            options: std.fmt.FormatOptions,
            writer: anytype,
        ) !void {
            _ = fmt;
            _ = options;

            try writer.writeAll("event.Buf{\n");
            try writer.print(
                "\t{}\n",
                .{std.fmt.fmtSliceHexUpper(self.bytes.asSlice())},
            );
            try writer.print("\t{} events\n", .{self.n_events});
            try writer.writeAll("}\n");
        }
    };
};

pub const Address = extern struct {
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

    const zero = Address{ .words = .{ 0, 0 } };
};

comptime {
    const alignment = @alignOf(Address);
    debug.assert(alignment == 8);
}

const Msg = struct {
    const Inner = union(enum) { sync_res: []Event };

    inner: Inner,
    origin: Address,
};

pub const TestStorage = struct {
    bytes: Vec(u8),

    pub fn init(buf: []u8) @This() {
        return .{ .bytes = Vec(u8).init(buf) };
    }

    pub fn append(self: *@This(), data: []const u8) !void {
        try self.bytes.pushSlice(data);
    }

    pub fn read(
        self: @This(),
        buf: *Vec(u8),
        region: Region,
    ) error{NotEnoughMem}!void {
        const data = try region.readSlice(self.bytes.asSlice());
        try buf.pushSlice(data);
    }
};

test "let's write some bytes" {
    const bytes_buf = try testing.allocator.alloc(u8, 127);
    defer testing.allocator.free(bytes_buf);
    var buf = Event.Buf.init(bytes_buf);

    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.Random.Pcg.init(seed);
    const id = Address.init(std.Random.Pcg, &rng);

    const evt = Event{
        .id = .{ .origin = id, .logical_pos = 0 },
        .payload = "j;fkls",
    };

    try buf.append(&evt);

    var it = buf.iter();

    while (try it.next()) |e| {
        try testing.expectEqualSlices(u8, evt.payload, e.payload);
    }

    const actual = buf.read(StorageOffset.zero);

    try testing.expectEqualDeep(actual, evt);
}

test "enqueue, commit and read data" {
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.Random.Pcg.init(seed);
    const addr = Address.init(std.Random.Pcg, &rng);

    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const storage = TestStorage.init(try allocator.alloc(u8, 272));
    const heap_mem = .{
        .enqd_events = try allocator.alloc(u8, 136),
        .enqd_offsets = try allocator.alloc(StorageOffset, 3),
        .cmtd_offsets = try allocator.alloc(StorageOffset, 5),
        .cmtd_acqs = try allocator.alloc(Address, 1),
    };

    var log = try Log(TestStorage).init(addr, storage, heap_mem);
    var read_buf = Event.Buf.init(try allocator.alloc(u8, 136));

    {
        const line = "I have known the arcane law";
        try testing.expectEqual(64, log.enqueue(line));
        try testing.expectEqual(1, log.commit());
        try log.readFromEnd(1, &read_buf);
        const actual = read_buf.read(StorageOffset.zero).payload;
        try testing.expectEqualSlices(u8, line, actual);
    }

    {
        const line = "On strange roads, such visions met";
        try testing.expectEqual(72, log.enqueue(line));
        try testing.expectEqual(1, log.commit());
        try log.readFromEnd(1, &read_buf);
        var it = read_buf.iter();
        const actual = (try it.next()).?.payload;
        try testing.expectEqualSlices(u8, line, actual);
    }

    // Read multiple things from the buffer
    {
        try log.readFromEnd(2, &read_buf);
        var it = read_buf.iter();

        try testing.expectEqualSlices(
            u8,
            "I have known the arcane law",
            (try it.next()).?.payload,
        );

        try testing.expectEqualSlices(
            u8,
            "On strange roads, such visions met",
            (try it.next()).?.payload,
        );
    }

    // Bulk commit two things
    {
        try testing.expectEqual(64, log.enqueue("That I have no fear, nor concern"));
        try testing.expectEqual(
            136,
            log.enqueue("For dangers and obstacles of this world"),
        );
        try testing.expectEqual(log.commit(), 2);

        try log.readFromEnd(2, &read_buf);
        var it = read_buf.iter();

        try testing.expectEqualSlices(
            u8,
            "That I have no fear, nor concern",
            (try it.next()).?.payload,
        );

        try testing.expectEqualSlices(
            u8,
            "For dangers and obstacles of this world",
            (try it.next()).?.payload,
        );
    }
}
