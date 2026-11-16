class A {
    void f() {
        x = a + b * c - d / e % f;
        x = a || b && c | d ^ e & f;
        x = a == b != c;
        x = a < b | c > d | e <= f | g >= h;
        x = a << 2 >> 3 >>> 4;
        x = -a + +b - -c;
        x = !a && ~b;
        x = ++a + --b;
        x = a++ + b--;
        x = a ? b : c ? d : e;
        x = a ? b = 1 : c;
        x = cond ? 1 : 2;
        x = (a + b) * c;
        x += 1; x -= 1; x *= 2; x /= 2; x %= 3;
        x &= 1; x |= 2; x ^= 3;
        x <<= 1; x >>= 1; x >>>= 1;
        x = y = z;
        a = b >= c;
        a = b > c;

        i = a > b ? a : b;
    }
}
