const std = @import("std");
const testing = std.testing;
const Allocator = std.mem.Allocator;

const mem = struct {
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
};

pub fn JaggedArray(comptime T: type) type {
    return struct {
        flat_mem: std.ArrayList(T),
        indices: std.ArrayList(mem.Region(T)),
        allocator: Allocator,

        pub fn init(allocator: Allocator) !@This() {
            return @This(){
                .flat_mem = std.ArrayList(T).init(allocator),
                .indices = std.ArrayList(mem.Region(u8)).init(allocator),
                .allocator = allocator,
            };
        }

        pub fn deinit(self: *@This()) void {
            self.flat_mem.deinit();
            self.indices.deinit();
            self.* = undefined;
        }

        fn append(self: *@This(), items: []const T) Allocator.Error!void {
            try self.indices.append(.{
                .pos = self.flat_mem.items.len,
                .len = items.len,
            });
            try self.flat_mem.appendSlice(items);
        }

        fn count(self: *@This()) usize {
            return self.indices.items.len;
        }

        fn get(self: @This(), index: usize) []const T {
            const region = self.indices.items[index];
            return region.read(self.flat_mem.items);
        }
    };
}

test "messing around with jagged arrays" {
    var ja = try JaggedArray(u8).init(testing.allocator);
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

const Buf = struct {
    buf: []u8,
    a: mem.Region(u8),
    b: mem.Region(u8),

    pub fn init(allocator: Allocator, capacity: usize) !@This() {
        return Buf{
            .buf = try allocator.alloc(u8, capacity),
            .a = mem.Region(u8).zero(),
            .b = mem.Region(u8).zero(),
        };
    }

    pub fn deinit(self: *@This(), allocator: Allocator) void {
        allocator.free(self.buf);
        self.* = undefined;
    }

    pub fn append_slice(self: *@This()) void {
        _ = self;
    }

    // TODO: delete below, just inline
    pub fn read_a(self: @This()) []const u8 {
        return self.a.read(self.buf);
    }

    pub fn read_b(self: @This()) []const u8 {
        return self.b.read(self.buf);
    }
};

test "alloc and free buf" {
    var buf = try Buf.init(testing.allocator, 8);
    defer buf.deinit(testing.allocator);
    try testing.expectEqualSlices(u8, buf.read_a(), &[_]u8{});
}
