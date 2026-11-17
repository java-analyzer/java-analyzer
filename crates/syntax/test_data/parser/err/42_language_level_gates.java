// release=14
sealed class Level14Gates permits Sub {
    record Point(int x, int y) {}

    String tb = """
        hello
        """;

    int test(Object o) {
        int _ = 1;
        if (o instanceof String s) {}
        if (o instanceof Point(int x, int y)) {}
        return switch (o) {
            case String s when !s.isEmpty() -> 1;
            default -> 0;
        };
    }
}
