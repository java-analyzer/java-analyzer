class A {
    A() {}
    public void run() { if (x) { y(); } }
    abstract int size();
    <T> T pick(T a, T b) throws Exception { return a; }
}
