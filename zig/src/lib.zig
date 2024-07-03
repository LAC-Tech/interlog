const std = @import("std");

const mem = struct {
    pub const Word = u64;
};

pub fn FixVecAligned(comptime T: type, comptime alignment: ?u29) type {
    return struct {
        _items: Slice,
        len: usize,

        pub const Slice = if (alignment) |a| ([]align(a) T) else []T;

        pub const Err = error{Overflow};

        pub fn init(allocator: std.mem.Allocator, _capacity: usize) !@This() {
            return @This(){
                ._items = try allocator.alignedAlloc(T, alignment, _capacity),
                .len = 0,
            };
        }

        pub fn deinit(self: *@This(), allocator: std.mem.Allocator) void {
            allocator.free(self._items);
            self.* = undefined;
        }

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
            return .{ ._items = slice, .len = slice.len };
        }
    };
}

pub fn FixVec(comptime T: type) type {
    return FixVecAligned(T, null);
}

test "fixvec stuff" {
    var fv = try FixVec(u64).init(std.testing.allocator, 8);
    defer fv.deinit(std.testing.allocator);

    try std.testing.expectEqual(fv.capacity(), 8);
    try fv.append(42);
    try std.testing.expectEqual(fv.len, 1);

    try fv.appendSlice(&[_]u64{ 6, 1, 9 });
    try std.testing.expectEqual(fv.len, 4);
}
pub const Addr = struct {
    words: [2]u64,
    pub fn init(comptime R: type, rng: *R) @This() {
        return .{
            .words = .{ rng.random().int(u64), rng.random().int(u64) },
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

        try writer.print("{x}{x}", .{ self.words[0], self.words[1] });
    }

    const zero = Addr{ .words = .{ 0, 0 } };
};

comptime {
    const alignment = @alignOf(Addr);
    std.debug.assert(alignment == 8);
}

/// In-memory mapping of event IDs to storage offsets
/// This keeps the following invariants:
/// - events must be stored consecutively per address,
///
/// Effectively stores the causal histories over every addr
const Index = struct {
    // One elem per address
    const Entry = struct {
        addr: Addr,
        // indices waiting to be written to the index
        txn: FixVec(usize),
        // indices that already exist in the index
        actual: FixVec(usize),

        fn init(
            allocator: std.mem.Allocator,
            addr: Addr,
            config: Config,
        ) !@This() {
            return .{
                .addr = addr,
                .txn = try FixVec(usize).init(
                    allocator,
                    config.txn_events_per_addr,
                ),
                .actual = try FixVec(usize).init(
                    allocator,
                    config.actual_events_per_addr,
                ),
            };
        }
    };

    const Config = struct {
        max_addrs: usize,
        txn_events_per_addr: usize,
        actual_events_per_addr: usize,
    };

    const InsertErr = error{
        NonConsecutivePos,
        IndexStartsAtNonZero,
        Overrun,
        NonMonotonicStorageOffset,
        Overflow,
    };

    // TODO: how many addresses is a good upper bound? This means linear search
    table: FixVec(Entry),
    config: Config,

    fn init(allocator: std.mem.Allocator, config: Config) !@This() {
        const table = try allocator.alloc(Entry, config.max_addrs);

        for (table) |*entry| {
            entry.* = try Entry.init(allocator, Addr.zero, config);
        }

        return .{
            .table = FixVec(Entry).fromSlice(table),
            .config = config,
        };
    }

    fn insert(
        self: *@This(),
        eid: Event.ID,
        storage_offset: usize,
    ) InsertErr!void {
        var existing = null;

        // TODO: linear search each time. sorted array + binary search??
        for (self.table) |entry| {
            if (entry.addr == eid.addr) {
                existing = entry;
                break;
            }
        }

        if (existing) |entry| {
            if (entry.txn.items.len != eid.pos) {
                return error.NonConsecutivePos;
            }

            if (existing.txn.getLastOrNull()) |last_offset| {
                if (last_offset + 1 != storage_offset) {
                    return error.NonMonotonicStorageOffset;
                }
            }

            try existing.txn.push(storage_offset);
        } else if (eid.pos != 0) {
            return error.IndexStartsAtNonZero;
        }

        // Storing a brand new entry.
        // At first zero addr, create entry with address eid.origin
        // write 0 to the txn buf therein
        @panic("TODO: implement above comment");
    }
};

test "construct index" {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    _ = try Index.init(
        arena.allocator(),
        .{
            .max_addrs = 100,
            .txn_events_per_addr = 10,
            .actual_events_per_addr = 1000,
        },
    );
}

pub const Actor = struct {
    addr: Addr,

    pub fn init(addr: Addr) @This() {
        return .{ .addr = addr };
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

const Event = struct {
    const ID = struct { origin: Addr, logical_pos: usize };

    id: ID,
    payload: []mem.Word,
};

const Msg = struct {
    const Inner = union(enum) { sync_res: []Event };

    inner: Inner,
    origin: Addr,
};
