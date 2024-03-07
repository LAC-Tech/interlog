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
    };
};

const Buf = struct {
    mem: []u8,
    a: mem.Region,
    b: mem.Region,

    pub fn init(allocator: Allocator, capacity: usize) !@This() {
        return Buf{
            .mem = try allocator.alloc(u8, capacity),
            .a = mem.Region.zero(),
            .b = mem.Region.zero(),
        };
    }

    pub fn deinit(self: *@This(), allocator: Allocator) void {
        allocator.free(self.mem);
        self.* = undefined;
    }
};

export fn add(a: i32, b: i32) i32 {
    return a + b;
}

test "alloc and free buf" {
    var buf = try Buf.init(testing.allocator, 8);
    defer buf.deinit(testing.allocator);
    try testing.expect(add(3, 7) == 10);
}
