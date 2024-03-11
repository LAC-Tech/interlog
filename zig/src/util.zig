const std = @import("std");
const testing = std.testing;
const Allocator = std.mem.Allocator;

pub fn Region(comptime T: type) type {
    return struct {
        pos: usize,
        len: usize,

        pub fn init(pos: usize, len: usize) @This() {
            return @This(){ .pos = pos, .len = len };
        }

        pub fn zero() @This() {
            return init(0, 0);
        }

        fn end(self: @This()) usize {
            return self.pos + self.len;
        }

        pub fn lengthen(self: *@This(), len_diff: usize) void {
            self.len += len_diff;
            self.end = self.pos + self.len;
        }

        pub fn read(self: @This(), bytes: []const T) []const T {
            return bytes[self.pos..self.end()];
        }

        pub fn write(self: *@This(), dest: []T, src: []const T) void {
            @memcpy(dest[self.pos..self.end()], src);
        }

        pub fn empty(self: *@This()) bool {
            return self.len == 0;
        }
    };
}

pub fn JaggedArray(comptime T: type) type {
    return struct {
        flat_mem: std.ArrayList(T),
        indices: std.ArrayList(Region(T)),

        pub fn init(allocator: Allocator) @This() {
            return @This(){
                .flat_mem = std.ArrayList(T).init(allocator),
                .indices = std.ArrayList(Region(T)).init(allocator),
            };
        }

        pub fn deinit(self: *@This()) void {
            self.flat_mem.deinit();
            self.indices.deinit();
            self.* = undefined;
        }

        pub fn append(self: *@This(), items: []const T) Allocator.Error!void {
            try self.indices.append(.{
                .pos = self.flat_mem.items.len,
                .len = items.len,
            });
            try self.flat_mem.appendSlice(items);
        }

        pub fn count(self: *@This()) usize {
            return self.indices.items.len;
        }

        pub fn get(self: @This(), index: usize) []const T {
            const region = self.indices.items[index];
            return region.read(self.flat_mem.items);
        }
    };
}

test "messing around with jagged arrays" {
    var ja = JaggedArray(u8).init(testing.allocator);
    defer ja.deinit();

    try testing.expectEqual(ja.count(), 0);

    try ja.append("9PM");
    try testing.expectEqual(ja.count(), 1);
    try testing.expectEqualSlices(u8, ja.get(0), "9PM");

    try ja.append("Til");
    try testing.expectEqual(ja.count(), 2);
    try testing.expectEqualSlices(u8, ja.get(1), "Til");

    try ja.append("I");
    try testing.expectEqual(ja.count(), 3);
    try testing.expectEqualSlices(u8, ja.get(2), "I");

    try ja.append("Come");
    try testing.expectEqual(ja.count(), 4);
    try testing.expectEqualSlices(u8, ja.get(3), "Come");
}

pub fn FixVec(comptime T: type) type {
    return struct {
        _items: []T,
        len: usize,

        pub fn init(allocator: Allocator, fixed_capacity: usize) !@This() {
            return @This(){
                ._items = try allocator.alloc(T, fixed_capacity),
                .len = 0,
            };
        }

        pub fn deinit(self: *@This(), allocator: Allocator) void {
            allocator.free(self._items);
            self.* = undefined;
        }

        pub fn capacity(self: *@This()) usize {
            return self._items.len;
        }
    };
}

test "fixvec stuff" {
    var fv = try FixVec(u64).init(testing.allocator, 8);
    defer fv.deinit(testing.allocator);

    try testing.expectEqual(fv.capacity(), 8);
}
