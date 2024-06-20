const std = @import("std");

fn Actor(comptime Addr: type) type {
    return struct {
        addr: Addr,

        fn init(addr: Addr) @This() {
            return .{ .addr = addr };
        }
    };
}

const sim_config = .{
    .max_actors = 256,
};

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    const allocator = arena.allocator();
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.rand.DefaultPrng.init(seed);

    const n_actors = rng.random().uintLessThan(usize, sim_config.max_actors);

    var actors = try allocator.alloc(Actor(u64), n_actors);

    for (0..n_actors) |i| {
        actors[i] = Actor(u64).init(rng.random().int(u64));
    }

    std.debug.print("seed = {d}\n", .{seed});
    std.debug.print("Running simulation with:\n", .{});
    std.debug.print("- {d} actors\n", .{actors.len});
}

test "simple test" {
    var list = std.ArrayList(i32).init(std.testing.allocator);
    defer list.deinit();
    try list.append(42);
    try std.testing.expectEqual(@as(i32, 42), list.pop());
}
