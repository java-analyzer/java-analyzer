class A {
    void f() {
        x = (int) y;
        x = (long) -y;
        x = (String) obj;
        x = (List<String>) obj;
        x = (int[]) obj;
        x = (Foo.Bar) obj;
        x = (Runnable & Serializable) () -> {};
        x = (a) + b;
        x = (a) - b;
        x = (a) * b;
        x = (a < b);
        x = (a) ? b : c;
        x = (a) == b;
        x = (T) (U) obj;
        x = (T) !flag;
        x = (T) new Foo();
        x = (T) this;
        x = ((Foo) obj).bar();
        x = (Foo) obj.bar();
        x = (byte) 1 + 2;
        x = (a + b) * (c + d);
        x = (a);
    }
}
