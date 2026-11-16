class A extends B {
    A() { this(1); }
    A(int x) { super(x); }
    A(int x, int y) { this.x = x; super.y = y; }
    void f() {
        this.g();
        super.g();
        Outer.this.g();
        Outer.super.g();
        Object o = this;
        boolean b1 = x instanceof Foo;
        boolean b2 = x instanceof Foo f;
        boolean b3 = x instanceof Foo f && f.ok();
        boolean b4 = x instanceof final Foo f;
        boolean b5 = x instanceof Point(int a, int b);
        boolean b6 = x instanceof Point(var a, Point(int c, int d)) && a > c;
        boolean b7 = !(x instanceof Foo);
        boolean b8 = x instanceof List<?> l;
        boolean b9 = x instanceof Foo ? true : false;
    }
}
