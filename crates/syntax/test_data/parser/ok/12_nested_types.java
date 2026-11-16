class Outer {
    static class Inner {}
    interface Callback {}
    enum Kind { A, B }
    record Pair(int a, int b) {}
    @interface Marker {}
    private final class Deep { class Deeper {} }
}
