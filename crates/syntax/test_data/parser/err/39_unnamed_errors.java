class UnnamedErrors {
    int _ = 1;
    void m(int _);
    void a() { x = _; }
    void b(Object o) { if (o instanceof _) {} }
    void c(Object o) { switch (o) { case _ -> a(); } }
    void ok() {}
}
