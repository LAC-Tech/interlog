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
};

comptime {
    const alignment = @alignOf(Addr);
    std.debug.assert(alignment == 8);
}

pub const Actor = struct {
    addr: Addr,

    pub fn init(addr: Addr) @This() {
        return .{ .addr = addr };
    }
};

const Event = struct {
    const ID = struct { origin: Addr, logical_pos: usize };

    id: ID,
    payload: []mem.Word,
};

const Msg = struct {
    const Inner = union(enum) { sync_res: []Event };
};
