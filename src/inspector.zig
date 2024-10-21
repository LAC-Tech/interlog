const core = @import("./core.zig");

const c = @cImport({
    @cInclude("ncurses.h");
});

const error_color_pair: c_short = 1;

pub fn render(err: core.Err, core_stats: core.Stats) void {
    _ = c.initscr();
    _ = c.start_color();

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
