class A {
    void f() {
        ;
        x = 1;
        foo();
        a.b.c(1, 2).d();
        this.x = y;
        i++;
        new Thing().run();
        return;
    }
    int g() { return a + b * 2; }
    void h() { throw new IllegalStateException("no"); }
    void i() {
        outer:
        loop: break outer;
        continue loop;
        break;
        continue;
        assert x > 0;
        assert x > 0 : "message " + x;
        { { } }
        synchronized (this) { count++; }
    }
}
