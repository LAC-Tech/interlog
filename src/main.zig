//! Deterministic Simulation Test

const std = @import("std");
const debug = std.debug;
const assert = std.debug.assert;
const RNG = std.Random.Xoshiro256;

const inspector = @import("inspector.zig");
const sim = @import("sim.zig");
const config = sim.config;
const lib = @import("lib.zig");
const core = lib.core;
const TestStorage = lib.TestStorage;
const HeapMemory = lib.HeapMemory;
const StorageOffset = lib.StorageOffset;
const Log = lib.Log;
const ReadBuf = lib.ReadBuf;

const MAX_SIM_TIME: u64 = 1000 * 60 * 60 * 24; // One day in MS

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const allocator = gpa.allocator();

    var args = std.process.args();
    _ = args.skip();
    const user_supplied_seed = args.next();

    const seed = if (user_supplied_seed) |s|
        try std.fmt.parseInt(u64, s, 16)
    else
        std.crypto.random.int(u64);
    var rng = std.rand.DefaultPrng.init(seed);

    const n_actors = config.n_actors.gen(RNG, &rng);

    var envs = std.AutoHashMap(core.Address, sim.Env).init(allocator);

    var stats = sim.Stats.init();

    for (0..n_actors) |_| {
        const env = try sim.Env.init(RNG, &rng, allocator);
        try envs.putNoClobber(env.log.addr, env);
    }

    debug.print("seed = {X}\n", .{seed});
    debug.print("Running simulation with:\n", .{});
    debug.print("- {d} actors\n", .{envs.count()});

    var payload_buf: [config.payload_size.at_most]u8 = undefined;
    var payload_lens = try std.ArrayListUnmanaged(usize).initCapacity(
        allocator,
        config.msg_len.at_most,
    );

    var sim_time: u64 = 0;
    while (sim_time < MAX_SIM_TIME) : (sim_time += 10) {
        var it = envs.valueIterator();

        while (it.next()) |env| {
            payload_lens.clearRetainingCapacity();
            env.payload_src.popPayloadLens(&payload_lens);

            for (payload_lens.items) |payload_len| {
                const payload = payload_buf[0..payload_len];
                rng.fill(payload);
                _ = env.log.enqueue(payload) catch |err| {
                    inspector.render(err, env.log.stats());
                    debug.print("ENQUEUE ERR\n", .{});
                    debug.print("Sim time: {}\n", .{sim_time});
                    debug.print("Sim stats: {}\n", .{stats});
                    debug.print("Log stats: {}\n", .{env.log.stats()});
                    return err;
                };
            }

            assert(payload_lens.items.len == try env.log.commit());
            stats.update(payload_lens.items.len);
        }
    }

    debug.print("Stats: {}\n", .{stats});
    debug.print("simulation has successfully completed!\n", .{});
}

test "set up and tear down simulated env" {
    const seed: u64 = std.crypto.random.int(u64);
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    var rng = std.Random.Pcg.init(seed);
    const allocator = arena.allocator();
    _ = try sim.Env.init(std.Random.Pcg, &rng, allocator);
}
