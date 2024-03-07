const std = @import("std");
const testing = std.testing;
const Allocator = std.mem.Allocator;

const mem = struct {
    const Region = struct {
        pos: usize,
        len: usize,
        end: usize,

        pub fn init(pos: usize, len: usize) @This() {
            return @This(){ .pos = pos, .len = len, .end = pos + len };
        }

        pub fn zero() @This() {
            return init(0, 0);
        }

        pub fn lengthen(self: *@This(), len_diff: usize) void {
            self.len += len_diff;
            self.end = self.pos + self.len;
        }

        pub fn read(self: @This(), bytes: []const u8) []const u8 {
            return bytes[self.pos..self.end];
        }

        pub fn write(self: *@This(), dest: []u8, src: []const u8) void {
            @memcpy(dest[self.pos..self.end], src);
        }

        pub fn empty(self: *@This()) bool {
            return self.len == 0;
        }
    };
};

const Buf = struct {
    buf: []u8,
    a: mem.Region,
    b: mem.Region,

    pub fn init(allocator: Allocator, capacity: usize) !@This() {
        return Buf{
            .buf = try allocator.alloc(u8, capacity),
            .a = mem.Region.zero(),
            .b = mem.Region.zero(),
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
