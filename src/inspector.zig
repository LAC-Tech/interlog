//! The inspector is a CLI repl, which we can use to inspect logs.
//! It can be thought of as a very basic VM, with one 'register': a log address
//! Commands are polish notation:
//! - a <addr> : set address register to <addr>
//! - ls : list all logs
//! - rh <n> : n most recent events of current address, payloads as hex
//! - rs <n> : n most recent events of current address, payloads as strings

const core = @import("./core.zig");

var current: core.States = undefined;

pub fn render(err: core.Err, addr: Address, core_stats: core.Stats) void {
    current = core_stats;

    // Error prefix
    {
        _ = c.init_pair(error_color_pair, c.COLOR_RED, c.COLOR_BLACK);
        _ = c.attron(c.COLOR_PAIR(error_color_pair));
        _ = c.printw("ERROR: ");
        _ = c.attroff(c.COLOR_PAIR(error_color_pair));
    }
    // TODO: if there's a way to render err codes directly, do that.
    const err_msg = switch (err) {
        error.NotEnoughMem => "Not Enough Memory\n",
        error.StorageOffsetNonMonotonic => "Storage Offset is Non-Monotonic\n",
        error.StorageOffsetNot8ByteAligned => "Storage Offset is not 8-byte aligned\n",
    };
    _ = c.printw(err_msg);
    _ = c.printw("");
    _ = core_stats;
    _ = c.refresh();
    _ = c.getch();
    _ = c.endwin();
}
