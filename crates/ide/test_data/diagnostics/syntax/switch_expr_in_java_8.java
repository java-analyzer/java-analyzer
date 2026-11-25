// release=8
class Main {
    int f(int x) {
        return switch (x) {
            case 1 -> 10;
            default -> 20;
        };
    }
}
