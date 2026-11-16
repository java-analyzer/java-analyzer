class A {
    void f() throws Exception {
        try { a(); } catch (Exception e) { b(); }
        try { a(); } finally { c(); }
        try { a(); } catch (IOException | RuntimeException e) { } catch (final Error e) { } finally { }
        try (Reader r = open()) { r.read(); }
        try (final Reader r = open(); var w = openWriter();) { }
        try (r) { }
        try (r; w) { } catch (Exception e) { }
        try (var in = new Scanner(System.in)) { } catch (@Anno Exception e) { }
    }
}
