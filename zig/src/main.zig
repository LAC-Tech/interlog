const sim = @import("sim.zig");
const lib = @import("lib.zig");
const std = @import("std");
const RNG = std.Random.Xoshiro256;

const MAX_SIM_TIME: u64 = 1000 * 60 * 60 * 24; // One day in MS

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const allocator = gpa.allocator();
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.rand.DefaultPrng.init(seed);

    const n_actors = sim.config.n_actors.gen(RNG, &rng);

    var envs = std.AutoHashMap(lib.Addr, sim.Env).init(allocator);

    for (0..n_actors) |_| {
        const env = try sim.Env.init(RNG, &rng, allocator);
        try envs.putNoClobber(env.log.addr, env);
    }

    std.debug.print("seed = {d}\n", .{seed});
    std.debug.print("Running simulation with:\n", .{});
    std.debug.print("- {d} actors\n", .{envs.count()});

    var payload_buf: [sim.config.payload_size.at_most]u8 = undefined;
    var payload_lens = try std.ArrayListUnmanaged(usize).initCapacity(
        allocator,
        sim.config.msg_len.at_most,
    );
    var i: u64 = 0;
    while (i < MAX_SIM_TIME) : (i += 10) {
        var it = envs.valueIterator();

        while (it.next()) |val| {
            payload_lens.clearRetainingCapacity();
            val.popPayloadLens(&payload_lens);

            std.debug.print("Sending actor {s} the following\n", .{val.log.addr});
            for (payload_lens.items) |payload_len| {
                const payload = payload_buf[0..payload_len];
                rng.fill(payload);
                std.debug.print("\t{}\n", .{std.fmt.fmtSliceHexLower(payload)});
            }
        }

        //std.debug.print("done", .{});

        // for each env, generate payload lens
        // loop over payload lens, fillin the payload buffer every time
        // as you fill the pay load buffer, enqueue it on the actor
        // once the loop is done, commit
    }

    std.debug.print("simulation has successfully completed!\n", .{});
}

test "set up and tear down simulated env" {
    const seed: u64 = std.crypto.random.int(u64);
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    var rng = std.Random.Pcg.init(seed);
    const allocator = arena.allocator();
    _ = try sim.Env.init(std.Random.Pcg, &rng, allocator);
}
