class A {
    int a, b = 2, c[], d[] = {1};
    private static final Map<String, List<Integer>> index = new HashMap<>();
    void run() {}
    abstract int size() throws IOException, SQLException;
    <T extends Comparable<T>> T max(T x, T y) { return x; }
    int legacy()[] { return null; }
    String join(String sep, String... parts) { return ""; }
    void modifiers(final int a, @Nullable String b, final @Anno long c) {}
    <T> A(T value) {}
    protected A(int x, int y) throws Exception { this(); }
    native void nativeCall();
    void trailing() {}
}
