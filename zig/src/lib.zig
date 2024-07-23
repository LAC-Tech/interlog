const std = @import("std");
const ArrayListUnmanaged = std.ArrayListUnmanaged;
const Allocator = std.mem.Allocator;

pub fn Log(comptime Storage: type) type {
    return struct {
        addr: Addr,
        enqueued: Enqueued,
        committed: Committed(Storage),

        pub fn init(
            allocator: Allocator,
            addr: Addr,
            config: Config,
        ) Allocator.Error!@This() {
            return .{
                .addr = addr,
                .enqueued = Enqueued{},
                .committed = try Committed(Storage).init(
                    allocator,
                    config.max_events,
                ),
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

const Config = struct {
    txn_size: usize,
    max_txn_events: usize,
    max_events: usize,
};

const Enqueued = struct {};

fn Committed(comptime Storage: type) type {
    return struct {
        offsets: ArrayListUnmanaged(usize),
        events: Storage,

        pub fn init(
            allocator: Allocator,
            max_events: usize,
        ) Allocator.Error!@This() {
            return .{
                .offsets = try ArrayListUnmanaged(usize).initCapacity(
                    allocator,
                    max_events,
                ),
                .events = Storage.init(),
            };
        }
    };
}

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
