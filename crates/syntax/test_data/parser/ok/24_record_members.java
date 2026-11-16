record Point(int x, int y) {
    Point {
        if (x < 0) { throw new IllegalArgumentException(); }
    }
    Point(int x) { this(x, 0); }
    static Point origin() { return new Point(0, 0); }
    double dist() { return Math.sqrt(x * x + y * y); }
}
record Tagged(@Nullable String tag, int... values) {}
record Empty() {}
