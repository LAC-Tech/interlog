const std = @import("std");
const RNG = std.Random.Xoshiro256;

const sim = @import("sim.zig");
const lib = @import("lib.zig");
const Addr = lib.Addr;
const TestStorage = lib.TestStorage;
const HeapMemory = lib.HeapMemory;
const StorageOffset = lib.StorageOffset;
const Log = lib.Log;
const ReadBuf = lib.ReadBuf;

const MAX_SIM_TIME: u64 = 1000 * 60 * 60 * 24; // One day in MS

pub fn main() !void {
    const seed: u64 = std.crypto.random.int(u64);
    var rng = std.Random.Pcg.init(seed);

    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const addr = Addr.init(std.Random.Pcg, &rng);
    const storage = TestStorage.init(try allocator.alloc(u8, 4096));
    const heap_memory = HeapMemory{
        .enqueued = .{
            .events = try allocator.alloc(u8, 4096),
            .offsets = try allocator.alloc(StorageOffset, 3),
        },
        .committed = .{
            .offsets = try allocator.alloc(StorageOffset, 3),
        },
    };

    var log = Log(TestStorage).init(addr, storage, heap_memory);

    var read_buf = ReadBuf.init(try allocator.alloc(u8, 128));
    log.enqueue("I have known the arcane law");
    try std.testing.expectEqual(log.commit(), 1);
    try log.readFromEnd(1, &read_buf);
    try std.testing.expectEqualSlices(
        u8,
        "I have known the arcane law",
        read_buf.read(StorageOffset.zero).payload,
    );

    log.enqueue("On strange roads, such visions met");
    try std.testing.expectEqual(log.commit(), 1);
    try log.readFromEnd(1, &read_buf);
    var it = read_buf.iter();
    const next = it.next();
    const actual = next.?.payload;
    try std.testing.expectEqualSlices(
        u8,
        "On strange roads, such visions met",
        //read_buf.read(StorageOffset.zero).payload,
        actual,
    );

    std.debug.print("{}\n", log.committed.offsets);
    try log.readFromEnd(2, &read_buf);
    it = read_buf.iter();

    try std.testing.expectEqualSlices(
        u8,
        "On strange roads, such visions met",
        it.next().?.payload,
    );

    try std.testing.expectEqualSlices(
        u8,
        "I have known the arcane law",
        it.next().?.payload,
    );

    //var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    //const allocator = gpa.allocator();
    //const seed: u64 = std.crypto.random.int(u64);
    //var rng = std.rand.DefaultPrng.init(seed);

    //const n_actors = sim.config.n_actors.gen(RNG, &rng);

    //var envs = std.AutoHashMap(lib.Addr, sim.Env).init(allocator);

    //for (0..n_actors) |_| {
    //    const env = try sim.Env.init(RNG, &rng, allocator);
    //    try envs.putNoClobber(env.log.addr, env);
    //}

    //std.debug.print("seed = {d}\n", .{seed});
    //std.debug.print("Running simulation with:\n", .{});
    //std.debug.print("- {d} actors\n", .{envs.count()});

    //var payload_buf: [sim.config.payload_size.at_most]u8 = undefined;
    //var payload_lens = try std.ArrayListUnmanaged(usize).initCapacity(
    //    allocator,
    //    sim.config.msg_len.at_most,
    //);
    //var i: u64 = 0;
    //while (i < MAX_SIM_TIME) : (i += 10) {
    //    var it = envs.valueIterator();

    //    while (it.next()) |val| {
    //        payload_lens.clearRetainingCapacity();
    //        val.popPayloadLens(&payload_lens);

    //        std.debug.print("Sending actor {s} the following\n", .{val.log.addr});
    //        for (payload_lens.items) |payload_len| {
    //            const payload = payload_buf[0..payload_len];
    //            rng.fill(payload);
    //            std.debug.print("\t{}\n", .{std.fmt.fmtSliceHexLower(payload)});
    //        }
    //    }

    //    //std.debug.print("done", .{});

    //    // for each env, generate payload lens
    //    // loop over payload lens, fillin the payload buffer every time
    //    // as you fill the pay load buffer, enqueue it on the actor
    //    // once the loop is done, commit
    //}

    //std.debug.print("simulation has successfully completed!\n", .{});
}

test "set up and tear down simulated env" {
    const seed: u64 = std.crypto.random.int(u64);
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    var rng = std.Random.Pcg.init(seed);
    const allocator = arena.allocator();
    _ = try sim.Env.init(std.Random.Pcg, &rng, allocator);
}
