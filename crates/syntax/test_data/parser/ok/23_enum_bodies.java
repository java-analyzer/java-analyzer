enum Empty {}
enum OnlySemicolon { ; }
enum Trailing { A, B, }
enum TrailingSemicolon { A, B, ; int x; }
enum Annotated { @Deprecated OLD, NEW }
enum WithBodies {
    ONE(1) { int value() { return 1; } },
    TWO(2, "two") { },
    THREE;

    private final int x;
    WithBodies() { this(0); }
    WithBodies(int x) { this.x = x; }
    WithBodies(int x, String s) { this(x); }
    int value() { return x; }
}
