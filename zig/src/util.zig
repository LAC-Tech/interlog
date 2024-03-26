const std = @import("std");
const testing = std.testing;
const Allocator = std.mem.Allocator;

// A region of memory with bounds checked functions to read and write from it
// Kind of turning pos..pos+len into an object, that can be grown, shifted etc
pub const Region = struct {
    pos: usize,
    len: usize,

    const WriteError = error{
        DestOverflow,
    };

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
    ) WriteError!void {
        if (self.end() > dest.len) {
            return error.DestOverflow;
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
    try testing.expectEqualSlices(u8, r.read(u8, &.{}).?, &.{});
}

test "empty region, non-empty slice" {
    const r = Region.zero();
    try testing.expectEqualSlices(u8, r.read(u8, &.{ 1, 3, 3, 7 }).?, &.{});
}

test "non-empty region, non-empty slice" {
    const r = Region.init(1, 2);
    try testing.expectEqualSlices(
        u8,
        r.read(u8, &.{ 1, 3, 3, 7 }).?,
        &.{ 3, 3 },
    );
}

pub fn JaggedArray(comptime T: type) type {
    return struct {
        flat_mem: std.ArrayList(T),
        indices: std.ArrayList(Region),

        pub fn init(allocator: Allocator) @This() {
            return @This(){
                .flat_mem = std.ArrayList(T).init(allocator),
                .indices = std.ArrayList(Region).init(allocator),
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

        pub fn get(self: @This(), index: usize) ?[]const T {
            const region = self.indices.items[index];
            return region.read(T, self.flat_mem.items);
        }
    };
}

test "messing around with jagged arrays" {
    var ja = JaggedArray(u8).init(testing.allocator);
    defer ja.deinit();

    try testing.expectEqual(ja.count(), 0);

    try ja.append("9PM");
    try testing.expectEqual(ja.count(), 1);
    try testing.expectEqualSlices(u8, ja.get(0).?, "9PM");

    try ja.append("Til");
    try testing.expectEqual(ja.count(), 2);
    try testing.expectEqualSlices(u8, ja.get(1).?, "Til");

    try ja.append("I");
    try testing.expectEqual(ja.count(), 3);
    try testing.expectEqualSlices(u8, ja.get(2).?, "I");

    try ja.append("Come");
    try testing.expectEqual(ja.count(), 4);
    try testing.expectEqualSlices(u8, ja.get(3).?, "Come");
}

pub fn FixVecAligned(comptime T: type, comptime alignment: ?u29) type {
    return struct {
        _items: Slice,
        len: usize,

        pub const Slice = if (alignment) |a| ([]align(a) T) else []T;

        pub const Err = error{Overflow};

        pub fn init(allocator: Allocator, fixed_capacity: usize) !@This() {
            return @This(){
                ._items = try allocator.alignedAlloc(T, alignment, fixed_capacity),
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
    };
}

pub fn FixVec(comptime T: type) type {
    return FixVecAligned(T, null);
}

test "fixvec stuff" {
    var fv = try FixVec(u64).init(testing.allocator, 8);
    defer fv.deinit(testing.allocator);

    try testing.expectEqual(fv.capacity(), 8);
    try fv.append(42);
    try testing.expectEqual(fv.len, 1);

    try fv.appendSlice(&[_]u64{ 6, 1, 9 });
    try testing.expectEqual(fv.len, 4);
}
