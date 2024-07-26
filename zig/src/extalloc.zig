/// Externally allocated data structures
///
/// My rationale here is I want all my heap allocated memory to be in one place.
/// So I can keep an eye on it!!!
const std = @import("std");
const err = @import("./err.zig");

// This could just be ArrayListAlignedUnManaged. However:
// - I can add conveniene functions like "last"
// - the logical length is not part of the slice, so I can pass empty buffers in
// TODO: just wrap it?
pub fn VecAligned(comptime T: type, comptime alignment: ?u29) type {
    return struct {
        _slice: Slice,
        len: usize,

        pub const Slice = if (alignment) |a| ([]align(a) T) else []T;

        pub fn capacity(self: *@This()) usize {
            return self.asSlice().len;
        }

        pub fn clear(self: *@This()) void {
            self.len = 0;
        }

        fn checkCapacity(self: *@This(), new_len: usize) err.Write!void {
            if (new_len > self.capacity()) {
                return error.Overrun;
            }
        }

        pub fn resize(self: *@This(), new_len: usize) err.Write!void {
            try self.checkCapacity(new_len);
            self.len = new_len;
        }

        pub fn append(self: *@This(), item: T) err.Write!void {
            try self.checkCapacity(self.len + 1);
            self._slice[self.len] = item;
            self.len += 1;
        }

        pub fn appendSlice(self: *@This(), items: []const T) err.Write!void {
            try self.checkCapacity(self.len + items.len);
            @memcpy(self._slice[self.len .. self.len + items.len], items);
            self.len += items.len;
        }

        // Note: the slice contents will be overwritten
        // It's treated as logically empty
        pub fn fromSlice(slice: Slice) @This() {
            return .{ ._slice = slice, .len = 0 };
        }

        pub fn asSlice(self: @This()) Slice {
            return self._slice[0..self.len];
        }

        pub fn last(self: @This()) ?T {
            return if (self.len == 0) null else self._slice[self.len - 1];
        }
    };
}

pub fn Vec(comptime T: type) type {
    return VecAligned(T, null);
}

test "fixvec stuff" {
    const buf = try std.testing.allocator.alloc(u64, 8);
    defer std.testing.allocator.free(buf);
    var fv = Vec(u64).fromSlice(buf);

    try std.testing.expectEqual(fv.capacity(), 8);
    try fv.append(42);
    try std.testing.expectEqual(fv.len, 1);

    try fv.appendSlice(&[_]u64{ 6, 1, 9 });
    try std.testing.expectEqual(fv.len, 4);
}
