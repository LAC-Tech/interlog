/// Things that ought to be in the std lib but aren't
pub fn sum(comptime T: type, ns: []const T) T {
    var result: T = 0;
    for (ns) |n| {
        result += n;
    }

    return result;
}
