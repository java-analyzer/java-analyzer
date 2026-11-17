class Unnamed {
    void m(Object o, List<String> xs) throws Exception {
        var _ = o.toString();
        int _ = 1;
        final String _ = "x", kept = "y";
        for (int _ = 0, i = 0; i < 1; i++) {}
        for (var _ : xs) {}
        for (final @A String _ : xs) {}
        try (var _ = open(); AutoCloseable _ = open()) {
        } catch (IllegalStateException | IllegalArgumentException _) {
        }
        Fn1 a = _ -> 1;
        Fn2 b = (_, y) -> y;
        Fn2 c = (int _, var y) -> y;
        Fn1 d = (final @A String _) -> 1;
        if (o instanceof String _) {}
        if (o instanceof Point(_, int y)) {}
        if (o instanceof Outer(Inner(_), _)) {}
        switch (o) {
            case Outer.Inner _ -> a();
            case byte[] _ -> b();
            case Point(Foo _, _) -> c();
            default -> {}
        }
    }
}
