const sim = @import("sim.zig");
const lib = @import("lib.zig");
const std = @import("std");
const RNG = std.Random.Xoshiro256;

const ONE_DAY_IN_MS: u64 = 1000 * 60 * 60 * 24;

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const allocator = gpa.allocator();
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.rand.DefaultPrng.init(seed);

    const n_actors = sim.config.n_actors.gen(RNG, &rng);

    var envs = std.AutoHashMap(lib.Addr, sim.Env).init(allocator);

    for (0..n_actors) |_| {
        const env = try sim.Env.init(RNG, &rng, allocator);
        try envs.putNoClobber(env.actor.addr, env);
    }

    std.debug.print("seed = {d}\n", .{seed});
    std.debug.print("Running simulation with:\n", .{});
    std.debug.print("- {d} actors\n", .{envs.count()});

    //var payload_buf: [sim.config.payload_size.at_most]u8 = undefined;
    var payload_lens = try std.ArrayListUnmanaged(usize).initCapacity(
        allocator,
        sim.config.msg_len.at_most,
    );
    var i: u64 = 0;
    while (i < ONE_DAY_IN_MS) : (i += 10) {
        var it = envs.iterator();

        while (it.next()) |entry| {
            payload_lens.clearRetainingCapacity();
            try entry.value_ptr.payload_src.popPayloadLens(&payload_lens);
            std.debug.print("{}", .{payload_lens});
        }

        // for each env, generate payload lens
        // loop over payload lens, fillin the payload buffer every time
        // as you fill the pay load buffer, enqueue it on the actor
        // once the loop is done, commit
    }
}

test "set up and tear down sim env" {
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.rand.DefaultPrng.init(seed);
    var env = try sim.Env.init(std.Random.Xoshiro256, &rng, std.testing.allocator);
    defer env.deinit(std.testing.allocator);
}
