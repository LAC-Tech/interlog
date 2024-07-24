const std = @import("std");
const err = @import("./err.zig");

// A region of memory with bounds checked functions for reading & writing
// Turns pos..pos+len into an object, that can be grown, shifted etc
pub const Region = struct {
    pos: usize,
    len: usize,

    pub fn init(pos: usize, len: usize) @This() {
        return @This(){ .pos = pos, .len = len };
    }

    pub fn zero() @This() {
        return init(0, 0);
    }

    pub fn end(self: @This()) usize {
        return self.pos + self.len;
    }

    pub fn lengthen(self: *@This(), len_diff: usize) void {
        self.len += len_diff;
    }

    pub fn read(self: @This(), comptime T: type, items: []const T) ?[]const T {
        if (self.end() > items.len) {
            return null;
        } else {
            return items[self.pos..self.end()];
        }
    }

    // "Safe" wrapper around memcpy
    pub fn write(
        self: @This(),
        dest: anytype,
        src: anytype,
    ) err.Write!void {
        if (self.end() > dest.len) {
            return error.Overrun;
        }

        @memcpy(dest[self.pos..self.end()], src);
    }

    pub fn empty(self: @This()) bool {
        return self.len == 0;
    }

    pub fn isZero(self: @This()) bool {
        return self.pos == 0 and self.len == 0;
    }

    pub fn changePos(self: *@This(), new_pos: usize) void {
        self.pos = new_pos;
        self.len = self.end() - new_pos;
    }
};

test "empty region, empty slice" {
    const r = Region.zero();
    try std.testing.expectEqualSlices(u8, r.read(u8, &.{}).?, &.{});
}

test "empty region, non-empty slice" {
    const r = Region.zero();
    try std.testing.expectEqualSlices(u8, r.read(u8, &.{ 1, 3, 3, 7 }).?, &.{});
}

test "non-empty region, non-empty slice" {
    const r = Region.init(1, 2);
    try std.testing.expectEqualSlices(
        u8,
        r.read(u8, &.{ 1, 3, 3, 7 }).?,
        &.{ 3, 3 },
    );
}
