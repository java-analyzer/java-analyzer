class A {
    int f(Object o, int x) {
        int a = switch (x) {
            case 1 -> 10;
            case 2, 3 -> 20;
            case 4 -> { yield 30; }
            default -> throw new IllegalStateException();
        };
        String s = switch (o) {
            case null -> "null";
            case Integer i when i > 10 -> "big";
            case Integer i -> "int";
            case String str -> str;
            case Point(int px, int py) -> "point " + px;
            case Color c when c == RED || c == BLUE -> "color";
            default -> "other";
        };
        switch (x) {
            case FOO -> a();
            case Outer.BAR -> b();
            case null, default -> c();
        }
        switch (x) {
            case 1:
                y = x > 0 ? 1 : 2;
                break;
            case A, B:
                break;
            default:
                break;
        }
        return switch (x) { case 1 -> 1; default -> 0; } + 1;
    }
}
