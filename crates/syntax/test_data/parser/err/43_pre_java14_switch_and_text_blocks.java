// release=11
class Level11Gates {
    String tb = """
        hello
        """;

    int f(int x) {
        return switch (x) {
            case 1 -> 10;
            default -> {
                yield 20;
            }
        };
    }
}
