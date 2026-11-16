class A extends B<String> implements C, D.E<F> {}
interface J extends K, L {}
enum E implements I {}
record R<T>(T value) implements Comparable<R<T>> {}
