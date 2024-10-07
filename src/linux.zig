const std = @import("std");

export const Storage = struct {
    const DirectIO = struct {
        // Syscalls
        const linux = std.os.linux;
        const open = linux.open;
        const pread = linux.pread;
        const write = linux.write;
        const close = linux.close;

        const O = linux.O;
        const S = linux.S;

        const flags = O.DIRECT | O.CREATE | O.APPEND | O.RDWR | O.DSYNC;
        const mode = S.IRUSR | S.IWUSR;

        fd: usize,

        fn init(path: [*:0]const u8) @This() {
            return .{ .fd = open(path, flags, mode) };
        }

        fn deinit(self: @This()) void {
            close(self.fd);
        }
    };
};
