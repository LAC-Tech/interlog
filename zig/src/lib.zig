const std = @import("std");

const mem = struct {
    pub const Word = u64;
};

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

    const max = Addr {.words = .{0, 0}};
};

comptime {
    const alignment = @alignOf(Addr);
    std.debug.assert(alignment == 8);
}

/// In-memory mapping of event IDs to disk offsets
/// This keeps the following invariants:
/// - events must be stored consecutively per address,
///
/// Effectively stores the causal histories over every addr
const Index = struct {
    // One elem per address
    const Elem = struct {
        addr: Addr,
        // indices waiting to be written to the index
        txn: std.ArrayListUnmanaged(usize),
        // indices that already exist in the index
        actual: std.ArrayListUnmanaged(usize),
        fn init(
            allocator: std.mem.Allocator,
            addr: Addr,
            config: Config,
        ) @This() {
            return .{
                .addr = addr,
                .txn = std.ArrayListUnmanaged(
                    allocator,
                    config.txn_events_per_addr,
                ),
                .actual = std.ArrayListUnmanaged(
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
    // TODO: how many addresses is a good upper bound? This means linear search
    table: std.ArrayListUnmanaged(Elem),
    config: Config,

    fn init(allocator: std.mem.Allocator, config: Config) @This() {

    }
};

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
