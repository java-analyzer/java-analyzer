// release=8
class Java8Identifiers {
    int _ = 1;

    int f(int _) {
        int local = _;
        yield(local);
        return _;
    }
}
