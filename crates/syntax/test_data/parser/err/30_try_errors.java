class A {
    void a() { try { } }
    void b() { try x(); }
    void c() { try { } catch { } }
    void d() { try { } catch (E) { } }
    void e() { try (r { } }
    void f() { try { } catch (E e) }
    void ok() {}
}
