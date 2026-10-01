export function check(x: number): number {
    if (x < 0) {
        throw new RangeError("negative");
    }
    return x;
}
